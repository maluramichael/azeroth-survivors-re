//! The co-op window on the hub: find a party on the LAN, host one, or join one by address.
//!
//! The window is a kit window ([`crate::kit`]) built into the pick screen like the collection
//! book: a dimmed full-screen layer, hidden until [`CoopUi::open`] (the hub's Co-op tab sets it).
//! Three tabs share it: "LAN games" (the parties [`LanGames`] found, one row each), "Host" (the
//! party's settings, then the address to give out and who is seated) and "Join by IP". It talks
//! to the session only through [`CoopCommand`] and the [`Session`] fields the commands read, and
//! asks [`LanSearch`] to search only while its LAN tab is in front.
//!
//! Every node is built once. The list has [`MAX_ROWS`] row slots that the refresh fills from the
//! found parties and hides when unused.

use std::net::{IpAddr, SocketAddr};

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::{FocusPolicy, RelativeCursorPosition};
use benilla_app::survivors_host::glue::art::{class_tc, tc_rect, GlueArt};
use benilla_app::survivors_host::glue::GlueVisuals;

use super::discovery::{LanGame, LanGames, LanSearch};
use super::{net, CoopCommand, Guest, Link, Session};
use crate::hud::{self, MenuRoot};
use crate::kit::{self, KitButton, KitCaption, KitFonts, KitStyle};
use crate::menu::MenuBuilt;
use crate::profile::Profile;
use crate::textfield::{self, TextField, TextFocus, TextSubmitted};
use crate::{data, CoopUi, Phase, Selection};

/// The window's size in the 768-unit canvas.
const PANEL_W: f32 = 900.0;
const PANEL_H: f32 = 640.0;
/// Party rows the list has room for; a longer list is cut.
const MAX_ROWS: usize = 12;
/// A party row's height, and its class icon.
const ROW_H: f32 = 54.0;
const ROW_ICON: f32 = 40.0;
/// Pixels a wheel notch scrolls the list.
const WHEEL_LINE: f32 = 28.0;
/// The most seats a party has, so the most seat lines the Host tab lists.
const MAX_SEATS: usize = 4;
/// Longest party name, password and address a field takes.
const NAME_LEN: usize = 24;
const PASS_LEN: usize = 24;
const ADDR_LEN: usize = 64;
/// The players a party of this size can have, as the Host tab offers them.
const SIZES: [u8; 3] = [2, 3, 4];

/// A row's fill: resting, under the mouse, and for a full party.
const ROW_FILL_HOT: Color = Color::srgba(0.16, 0.11, 0.05, 0.75);
const ROW_FILL_FULL: Color = Color::srgba(0.0, 0.0, 0.0, 0.2);
/// A row's class icon while the party is full.
const ICON_DIM: Color = Color::srgb(0.35, 0.35, 0.35);
/// The veil over the settings while a party is open.
const VEIL: Color = Color::srgba(0.04, 0.03, 0.02, 0.6);

/// The window's three tabs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Tab {
    #[default]
    Lan,
    Host,
    Join,
}

impl Tab {
    const ALL: [Tab; 3] = [Tab::Lan, Tab::Host, Tab::Join];

    fn label(self) -> &'static str {
        match self {
            Tab::Lan => "LAN games",
            Tab::Host => "Host",
            Tab::Join => "Join by IP",
        }
    }
}

/// The text fields, once built.
#[derive(Clone, Copy)]
struct Fields {
    party: Entity,
    host_pass: Entity,
    addr: Entity,
    join_pass: Entity,
}

/// What the window itself remembers.
#[derive(Resource, Default)]
struct LobbyView {
    tab: Tab,
    fields: Option<Fields>,
    /// What was typed in the last join, for the "Joining ..." line.
    joining: String,
    /// This machine's LAN address, read when the window opens (it opens a socket).
    ip: Option<IpAddr>,
    /// Whether the window was open last frame.
    was_open: bool,
}

/// The dimmed full-screen layer holding the window, hidden until opened.
#[derive(Component)]
struct LobbyPanel;
/// On [`MenuRoot`] once the window is built into it.
#[derive(Component)]
struct LobbyBuilt;

/// A button of the window.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
enum LobbyAction {
    Close,
    Tab(Tab),
    /// Clear the list; the next answers fill it again.
    Refresh,
    Public(bool),
    Size(u8),
    /// Open the party, or close it while it is open.
    Party,
    Join,
    Leave,
}

/// A party row's button, the index into the shown part of [`LanGames::games`].
#[derive(Component, Clone, Copy)]
struct LanRow(usize);

/// A box the mouse wheel scrolls.
#[derive(Component)]
struct LobbyScroll;

/// A text the refresh keeps current.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Txt {
    RowName(usize),
    RowSub(usize),
    RowZone(usize),
    RowCount(usize),
    /// The Host tab's hero line.
    Hero,
    /// The zone and difficulty under it.
    Where,
    /// "Hosting on 192.168.1.20:37373".
    Hosting,
    Seat(usize),
    /// The Join tab's note, or why joining is off.
    JoinNote,
    /// "Joining ..." and "In the party as player 2".
    JoinLine,
    /// `Session::status`.
    Status,
}

/// A node the refresh shows or hides.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Show {
    Body(Tab),
    Row(usize),
    Lock(usize),
    Dot(usize, usize),
    NoGames,
    HostingBlock,
    Seat(usize),
    Veil,
    /// The Join tab's fields and button, hidden while joining or joined.
    JoinForm,
    JoinStatus,
}

/// A node whose fill the refresh sets.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Paint {
    Row(usize),
    Dot(usize, usize),
}

/// A class icon.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Icon {
    Row(usize),
    Host,
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<LobbyView>().add_systems(
        Update,
        (build, show_panel, clicks, refresh_lan, refresh_state, scroll)
            .chain()
            // After the keys, which read Enter off the window flags first.
            .after(hud::keys)
            .before(GlueVisuals),
    );
}

/// Build the window into the pick screen once it is built.
fn build(
    mut commands: Commands,
    roots: Query<Entity, (With<MenuRoot>, With<MenuBuilt>, Without<LobbyBuilt>)>,
    fonts: Res<KitFonts>,
    mut view: ResMut<LobbyView>,
) {
    let Ok(root) = roots.single() else { return };
    let mut fields = None;
    commands
        .entity(root)
        .insert(LobbyBuilt)
        .with_children(|ui| {
            fields = Some(window(ui, &fonts));
        });
    view.fields = fields;
}

/// The window: heading, tabs, the three tab bodies, the status line and Close.
fn window(ui: &mut ChildSpawnerCommands, fonts: &KitFonts) -> Fields {
    let px = Val::Px;
    let mut out = None;
    ui.spawn((
        LobbyPanel,
        // Swallows the clicks on the pick screen under it.
        FocusPolicy::Block,
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            display: Display::None,
            ..default()
        },
    ))
    .with_children(|dim| {
        dim.spawn(kit::panel(Node {
            width: px(PANEL_W),
            height: px(PANEL_H),
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(px(24.0), px(12.0)),
            row_gap: px(6.0),
            ..default()
        }))
        .with_children(|win| {
            kit::heading(win, fonts, "Co-op", 26.0);

            // The tabs.
            win.spawn(Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                column_gap: px(8.0),
                ..default()
            })
            .with_children(|tabs| {
                for tab in Tab::ALL {
                    kit::button(
                        tabs,
                        fonts,
                        KitStyle::Tab,
                        tab.label(),
                        None,
                        Node {
                            width: px(200.0),
                            height: px(30.0),
                            ..default()
                        },
                        LobbyAction::Tab(tab),
                    );
                }
            });

            // The bodies, one shown at a time.
            win.spawn(Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: px(0.0),
                margin: UiRect::top(px(6.0)),
                ..default()
            })
            .with_children(|bodies| {
                lan_body(bodies, fonts);
                out = Some(host_and_join(bodies, fonts));
            });

            // What happened last, and Close.
            win.spawn((
                kit::body(fonts, "", 13.0, kit::CREAM),
                Txt::Status,
                Node {
                    min_height: px(18.0),
                    ..default()
                },
            ));
            win.spawn(Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            })
            .with_children(|foot| {
                kit::button(
                    foot,
                    fonts,
                    KitStyle::Normal,
                    "Close",
                    None,
                    Node {
                        width: px(150.0),
                        height: px(30.0),
                        ..default()
                    },
                    LobbyAction::Close,
                );
            });
        });
    });
    out.expect("the tab bodies are built")
}

/// A tab body's frame: fills the area, hidden until its tab is in front.
fn body_node(tab: Tab) -> impl Bundle {
    (
        Show::Body(tab),
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(8.0),
            display: Display::None,
            ..default()
        },
    )
}

/// The LAN tab: the hint with Refresh, then the list of parties.
fn lan_body(parent: &mut ChildSpawnerCommands, fonts: &KitFonts) {
    let px = Val::Px;
    parent.spawn(body_node(Tab::Lan)).with_children(|tab| {
        tab.spawn(Node {
            width: Val::Percent(100.0),
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|top| {
            top.spawn(kit::body(
                fonts,
                "Parties on your local network. Click one to join.",
                14.0,
                kit::CREAM,
            ));
            kit::button(
                top,
                fonts,
                KitStyle::Normal,
                "Refresh",
                None,
                Node {
                    width: px(120.0),
                    height: px(26.0),
                    ..default()
                },
                LobbyAction::Refresh,
            );
        });

        tab.spawn((
            LobbyScroll,
            RelativeCursorPosition::default(),
            kit::inset(Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: px(0.0),
                flex_direction: FlexDirection::Column,
                row_gap: px(4.0),
                padding: UiRect::all(px(6.0)),
                overflow: Overflow::scroll_y(),
                ..default()
            }),
        ))
        .with_children(|list| {
            list.spawn((
                Show::NoGames,
                Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::all(px(30.0)),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
            ))
            .with_children(|empty| {
                empty.spawn((
                    kit::body(
                        fonts,
                        "No parties found yet. The host must have its party set to public, and both machines must be on the same network.",
                        14.0,
                        kit::MUTED,
                    ),
                    TextLayout::new_with_justify(Justify::Center),
                    Node {
                        width: Val::Percent(100.0),
                        ..default()
                    },
                ));
            });
            for i in 0..MAX_ROWS {
                party_row(list, fonts, i);
            }
        });
    });
}

/// One party row: class icon, the names, a password chip, zone and seats.
fn party_row(list: &mut ChildSpawnerCommands, fonts: &KitFonts, i: usize) {
    let px = Val::Px;
    list.spawn((
        Button,
        LanRow(i),
        Show::Row(i),
        Paint::Row(i),
        kit::inset(Node {
            width: Val::Percent(100.0),
            height: px(ROW_H),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            column_gap: px(12.0),
            padding: UiRect::axes(px(8.0), px(4.0)),
            display: Display::None,
            ..default()
        }),
    ))
    .with_children(|row| {
        row.spawn((
            Icon::Row(i),
            ImageNode {
                color: Color::NONE,
                ..default()
            },
            Node {
                width: px(ROW_ICON),
                height: px(ROW_ICON),
                flex_shrink: 0.0,
                ..default()
            },
        ));
        row.spawn(Node {
            flex_grow: 1.0,
            min_width: px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|names| {
            names.spawn((kit::body(fonts, "", 18.0, kit::CREAM), Txt::RowName(i)));
            names.spawn((kit::body(fonts, "", 12.0, kit::MUTED), Txt::RowSub(i)));
        });
        row.spawn((
            Show::Lock(i),
            kit::inset(Node {
                padding: UiRect::axes(px(8.0), px(2.0)),
                display: Display::None,
                ..default()
            }),
        ))
        .with_children(|chip| {
            chip.spawn(kit::body(fonts, "Password", 11.0, kit::TITLE));
        });
        row.spawn(Node {
            width: px(190.0),
            flex_shrink: 0.0,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::FlexEnd,
            row_gap: px(3.0),
            ..default()
        })
        .with_children(|right| {
            right.spawn((kit::body(fonts, "", 14.0, kit::CREAM), Txt::RowZone(i)));
            right
                .spawn(Node {
                    align_items: AlignItems::Center,
                    column_gap: px(5.0),
                    ..default()
                })
                .with_children(|seats| {
                    seats.spawn((kit::body(fonts, "", 12.0, kit::MUTED), Txt::RowCount(i)));
                    for j in 0..usize::from(MAX_DOTS) {
                        seats.spawn((
                            Show::Dot(i, j),
                            Paint::Dot(i, j),
                            BackgroundColor(Color::NONE),
                            BorderColor::all(kit::PANEL_EDGE),
                            Node {
                                width: px(10.0),
                                height: px(10.0),
                                border: UiRect::all(px(1.0)),
                                border_radius: BorderRadius::all(Val::Percent(50.0)),
                                ..default()
                            },
                        ));
                    }
                });
        });
    });
}

/// The Host and Join tabs; returns their text fields.
fn host_and_join(parent: &mut ChildSpawnerCommands, fonts: &KitFonts) -> Fields {
    let (party, host_pass) = host_body(parent, fonts);
    let (addr, join_pass) = join_body(parent, fonts);
    Fields {
        party,
        host_pass,
        addr,
        join_pass,
    }
}

/// A small-caps label over a field.
fn labelled(
    parent: &mut ChildSpawnerCommands,
    fonts: &KitFonts,
    caption: &str,
    width: f32,
    field: impl FnOnce(&mut ChildSpawnerCommands) -> Entity,
) -> Entity {
    let mut out = Entity::PLACEHOLDER;
    parent
        .spawn(Node {
            width: Val::Px(width),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(3.0),
            ..default()
        })
        .with_children(|col| {
            col.spawn(kit::label(fonts, caption));
            out = field(col);
        });
    out
}

/// The Host tab: who hosts, what is open, the settings under a veil while it is, the button.
fn host_body(parent: &mut ChildSpawnerCommands, fonts: &KitFonts) -> (Entity, Entity) {
    let px = Val::Px;
    let mut fields = (Entity::PLACEHOLDER, Entity::PLACEHOLDER);
    parent.spawn(body_node(Tab::Host)).with_children(|tab| {
        // The "You host" box.
        tab.spawn(kit::inset(Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(12.0)),
            row_gap: px(8.0),
            ..default()
        }))
        .with_children(|boxed| {
            boxed
                .spawn(Node {
                    align_items: AlignItems::Center,
                    column_gap: px(12.0),
                    ..default()
                })
                .with_children(|hero| {
                    hero.spawn((
                        Icon::Host,
                        ImageNode {
                            color: Color::NONE,
                            ..default()
                        },
                        Node {
                            width: px(44.0),
                            height: px(44.0),
                            flex_shrink: 0.0,
                            ..default()
                        },
                    ));
                    hero.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        ..default()
                    })
                    .with_children(|lines| {
                        lines.spawn(kit::label(fonts, "You host"));
                        lines.spawn((kit::body(fonts, "", 18.0, kit::CREAM), Txt::Hero));
                        lines.spawn((kit::body(fonts, "", 13.0, kit::MUTED), Txt::Where));
                    });
                });
            // Once the party is open: the address to give out, and who sits at the table.
            boxed
                .spawn((
                    Show::HostingBlock,
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(4.0),
                        display: Display::None,
                        ..default()
                    },
                ))
                .with_children(|open| {
                    open.spawn((kit::body(fonts, "", 24.0, kit::TITLE), Txt::Hosting));
                    for i in 0..MAX_SEATS {
                        open.spawn((
                            kit::body(fonts, "", 14.0, kit::CREAM),
                            Txt::Seat(i),
                            Show::Seat(i),
                        ));
                    }
                });
        });

        // The settings; a veil over them while the party is open.
        tab.spawn(Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            row_gap: px(10.0),
            padding: UiRect::axes(px(4.0), px(2.0)),
            ..default()
        })
        .with_children(|set| {
            set.spawn(Node {
                column_gap: px(20.0),
                ..default()
            })
            .with_children(|row| {
                fields.0 = labelled(row, fonts, "Party name", 400.0, |col| {
                    textfield::spawn(
                        col,
                        fonts,
                        "",
                        "",
                        NAME_LEN,
                        false,
                        Node {
                            width: Val::Percent(100.0),
                            height: px(28.0),
                            ..default()
                        },
                    )
                });
                fields.1 = labelled(row, fonts, "Password (optional)", 300.0, |col| {
                    textfield::spawn(
                        col,
                        fonts,
                        "",
                        "",
                        PASS_LEN,
                        true,
                        Node {
                            width: Val::Percent(100.0),
                            height: px(28.0),
                            ..default()
                        },
                    )
                });
            });
            set.spawn(Node {
                align_items: AlignItems::Center,
                column_gap: px(8.0),
                ..default()
            })
            .with_children(|row| {
                row.spawn(Node {
                    width: px(120.0),
                    ..default()
                })
                .with_children(|l| {
                    l.spawn(kit::label(fonts, "Who can find it"));
                });
                for (public, caption) in [(true, "Public: in the LAN list"), (false, "Private: by address only")] {
                    kit::button(
                        row,
                        fonts,
                        KitStyle::Normal,
                        caption,
                        None,
                        Node {
                            width: px(230.0),
                            height: px(28.0),
                            ..default()
                        },
                        LobbyAction::Public(public),
                    );
                }
            });
            set.spawn(Node {
                align_items: AlignItems::Center,
                column_gap: px(8.0),
                ..default()
            })
            .with_children(|row| {
                row.spawn(Node {
                    width: px(120.0),
                    ..default()
                })
                .with_children(|l| {
                    l.spawn(kit::label(fonts, "Max players"));
                });
                for n in SIZES {
                    kit::button(
                        row,
                        fonts,
                        KitStyle::Normal,
                        &n.to_string(),
                        None,
                        Node {
                            width: px(56.0),
                            height: px(28.0),
                            ..default()
                        },
                        LobbyAction::Size(n),
                    );
                }
            });
            set.spawn(kit::body(
                fonts,
                "Friends join with the address below. Over the internet the host needs UDP port 37373 forwarded, or use Tailscale/ZeroTier.",
                12.0,
                kit::MUTED,
            ));
            // Over the settings, so a click does not reach them.
            set.spawn((
                Show::Veil,
                FocusPolicy::Block,
                BackgroundColor(VEIL),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0.0),
                    top: px(0.0),
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    display: Display::None,
                    ..default()
                },
            ));
        });

        tab.spawn(Node {
            flex_grow: 1.0,
            ..default()
        });
        tab.spawn(Node {
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|foot| {
            kit::button(
                foot,
                fonts,
                KitStyle::Primary,
                "Open party",
                None,
                Node {
                    width: px(280.0),
                    height: px(46.0),
                    ..default()
                },
                LobbyAction::Party,
            );
        });
    });
    fields
}

/// The Join tab: the address and password, the Join button; while joining, the status and Leave.
fn join_body(parent: &mut ChildSpawnerCommands, fonts: &KitFonts) -> (Entity, Entity) {
    let px = Val::Px;
    let mut fields = (Entity::PLACEHOLDER, Entity::PLACEHOLDER);
    parent.spawn(body_node(Tab::Join)).with_children(|tab| {
        tab.spawn((
            Show::JoinForm,
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: px(10.0),
                padding: UiRect::axes(px(4.0), px(2.0)),
                ..default()
            },
        ))
        .with_children(|form| {
            fields.0 = labelled(form, fonts, "Address", 460.0, |col| {
                textfield::spawn(
                    col,
                    fonts,
                    "192.168.1.20 or 192.168.1.20:37373",
                    "",
                    ADDR_LEN,
                    false,
                    Node {
                        width: Val::Percent(100.0),
                        height: px(28.0),
                        ..default()
                    },
                )
            });
            fields.1 = labelled(form, fonts, "Password", 300.0, |col| {
                textfield::spawn(
                    col,
                    fonts,
                    "if any",
                    "",
                    PASS_LEN,
                    true,
                    Node {
                        width: Val::Percent(100.0),
                        height: px(28.0),
                        ..default()
                    },
                )
            });
            form.spawn((kit::body(fonts, "", 13.0, kit::MUTED), Txt::JoinNote));
            form.spawn(Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                margin: UiRect::top(px(24.0)),
                ..default()
            })
            .with_children(|foot| {
                kit::button(
                    foot,
                    fonts,
                    KitStyle::Primary,
                    "Join",
                    None,
                    Node {
                        width: px(280.0),
                        height: px(46.0),
                        ..default()
                    },
                    LobbyAction::Join,
                );
            });
        });

        tab.spawn((
            Show::JoinStatus,
            kit::inset(Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                padding: UiRect::all(px(24.0)),
                row_gap: px(20.0),
                display: Display::None,
                ..default()
            }),
        ))
        .with_children(|status| {
            status.spawn((kit::body(fonts, "", 22.0, kit::TITLE), Txt::JoinLine));
            kit::button(
                status,
                fonts,
                KitStyle::Normal,
                "Leave",
                None,
                Node {
                    width: px(180.0),
                    height: px(32.0),
                    ..default()
                },
                LobbyAction::Leave,
            );
        });
    });
    fields
}

/// Most dots a party row shows, one per seat (the transport takes four players).
const MAX_DOTS: u8 = 4;

/// The class's name by `ClassDef::id`.
fn class_name(id: u8) -> &'static str {
    data::CLASSES
        .iter()
        .find(|c| c.id == id)
        .map_or("Adventurer", |c| c.name)
}

/// "<Name>'s party", the name a party goes by when the host typed none.
fn default_party_name(name: &str, class: &str) -> String {
    let name = name.trim();
    format!("{}'s party", if name.is_empty() { class } else { name })
}

/// "<Name> - <Class>", or just the class while the name is the class's own.
fn hero_line(name: &str, class: &str) -> String {
    let name = name.trim();
    if name.is_empty() || name == class {
        class.to_string()
    } else {
        format!("{name} - {class}")
    }
}

/// A LAN row's two lines: the party's name, and "<host> - <Class>" under it.
fn party_line(game: &LanGame) -> (String, String) {
    let class = class_name(game.class_id);
    let lobby = game.lobby.trim();
    let name = if lobby.is_empty() {
        default_party_name(&game.host, class)
    } else {
        lobby.to_string()
    };
    (name, format!("{} - {class}", game.host))
}

/// Whether a seat is free in the party.
fn joinable(game: &LanGame) -> bool {
    game.players < game.max_players
}

/// One flag per seat, filled for a player seated: `players` of `max` (at most [`MAX_DOTS`]).
fn dots(players: u8, max: u8) -> Vec<bool> {
    (0..max.min(MAX_DOTS)).map(|i| i < players).collect()
}

/// "Hosting on 192.168.1.20:37373", or the port alone when the address is not known.
fn hosting_line(ip: Option<IpAddr>, port: u16) -> String {
    match ip {
        Some(ip) => format!("Hosting on {}", SocketAddr::new(ip, port)),
        None => format!("Hosting on port {port}"),
    }
}

/// The seats of a hosted party for the Host tab: the host first, then the guests, then one
/// "Waiting for a player..." per free seat up to `max`. The flag is whether the seat is taken.
fn seat_lines(host: &str, guests: &[Guest], max: u8) -> Vec<(String, bool)> {
    let mut seats = vec![(host.to_string(), true)];
    for guest in guests {
        let class = class_name(guest.hello.class_id);
        seats.push((hero_line(&guest.hello.name, class), true));
    }
    while seats.len() < usize::from(max) {
        seats.push(("Waiting for a player...".to_string(), false));
    }
    seats
}

/// The Join tab's status line while joining or joined; `addr` is what was typed.
fn join_line(link: Link, player: Option<u8>, addr: &str) -> String {
    let addr = addr.trim();
    match (link, player) {
        (Link::Joining, _) if addr.is_empty() => "Joining the party...".to_string(),
        (Link::Joining, _) => format!("Joining {addr}..."),
        (Link::Joined, Some(p)) => format!("In the party as player {}", u32::from(p) + 1),
        (Link::Joined, None) => "In the party".to_string(),
        _ => String::new(),
    }
}


/// Close on Esc (unless a field has the keyboard) or when the pick screen goes; the display
/// follows [`CoopUi::open`], and the LAN search runs only while the LAN tab is in front.
fn show_panel(
    mut ui: ResMut<CoopUi>,
    mut view: ResMut<LobbyView>,
    mut search: ResMut<LanSearch>,
    mut focus: ResMut<TextFocus>,
    session: Res<Session>,
    phase: Res<State<Phase>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut panel: Query<&mut Node, With<LobbyPanel>>,
) {
    // A field handles its own Esc; it lets go of the keyboard at the end of the frame, so the
    // frame Esc is pressed in it `focus` is still set here.
    if ui.open && (*phase.get() != Phase::Menu || (keys.just_pressed(KeyCode::Escape) && focus.0.is_none())) {
        ui.open = false;
    }
    if ui.open && !view.was_open {
        // Opening: lead with where this machine stands, and ask the OS for its address once.
        view.tab = match session.link {
            Link::Hosting { .. } => Tab::Host,
            Link::Joining | Link::Joined => Tab::Join,
            Link::Off => view.tab,
        };
        view.ip = net::lan_ip();
    }
    if !ui.open && view.was_open && focus.0.is_some() {
        focus.0 = None;
    }
    if view.was_open != ui.open {
        view.was_open = ui.open;
    }
    let want = ui.open && view.tab == Tab::Lan;
    if search.active != want {
        search.active = want;
    }
    let display = if ui.open { Display::Flex } else { Display::None };
    for mut node in &mut panel {
        if node.display != display {
            node.display = display;
        }
    }
}

/// What a press asked for, carried out once the loops are done.
enum Ask {
    /// Open the party with the Host tab's settings.
    Host,
    /// Join the address typed on the Join tab.
    Join,
    /// Join the party in this list row.
    Row(usize),
}

/// A field's text, trimmed.
fn typed(fields: &Query<&mut TextField>, e: Entity) -> String {
    fields
        .get(e)
        .map(|f| f.value.trim().to_string())
        .unwrap_or_default()
}

/// The window's clicks and Enter in a field.
#[allow(clippy::too_many_arguments)]
fn clicks(
    phase: Res<State<Phase>>,
    mut ui: ResMut<CoopUi>,
    mut view: ResMut<LobbyView>,
    mut session: ResMut<Session>,
    mut lan: ResMut<LanGames>,
    mut focus: ResMut<TextFocus>,
    selection: Res<Selection>,
    mut submitted: MessageReader<TextSubmitted>,
    mut out: MessageWriter<CoopCommand>,
    actions: Query<(&Interaction, &LobbyAction, &KitButton), Changed<Interaction>>,
    rows: Query<(&Interaction, &LanRow), Changed<Interaction>>,
    mut fields: Query<&mut TextField>,
) {
    // Drain first, so an Enter typed while the window is shut is not answered when it opens.
    let submits: Vec<Entity> = submitted.read().map(|s| s.0).collect();
    if *phase.get() != Phase::Menu || !ui.open {
        return;
    }
    let Some(f) = view.fields else { return };
    let hosting = matches!(session.link, Link::Hosting { .. });
    let linked = matches!(session.link, Link::Joining | Link::Joined);

    let mut ask = None;
    for e in submits {
        if e == f.party || e == f.host_pass {
            ask = Some(Ask::Host);
        } else if e == f.addr || e == f.join_pass {
            ask = Some(Ask::Join);
        }
    }
    for (interaction, action, button) in &actions {
        if *interaction != Interaction::Pressed || button.disabled {
            continue;
        }
        match *action {
            LobbyAction::Close => {
                ui.open = false;
                return;
            }
            LobbyAction::Tab(tab) => {
                if view.tab != tab {
                    view.tab = tab;
                    focus.0 = None;
                }
            }
            // The next answers fill the list again, within a second.
            LobbyAction::Refresh => lan.games.clear(),
            LobbyAction::Public(public) => {
                if !hosting && session.lobby.public != public {
                    session.lobby.public = public;
                }
            }
            LobbyAction::Size(n) => {
                if !hosting && session.lobby.max_players != n {
                    session.lobby.max_players = n;
                }
            }
            LobbyAction::Party if hosting => {
                out.write(CoopCommand::Leave);
            }
            LobbyAction::Party => ask = Some(Ask::Host),
            LobbyAction::Join => ask = Some(Ask::Join),
            LobbyAction::Leave => {
                out.write(CoopCommand::Leave);
            }
        }
    }
    for (interaction, &LanRow(i)) in &rows {
        if *interaction == Interaction::Pressed {
            ask = Some(Ask::Row(i));
        }
    }

    let Some(ask) = ask else { return };
    let Some(class) = selection.class_def() else {
        session.status = "Pick a class on the hub first.".to_string();
        return;
    };
    if hosting || linked {
        // `run_commands` says it too, but a press that switched tabs first would hide it.
        if !matches!(ask, Ask::Host) || !hosting {
            session.status = "Leave the current party first.".to_string();
        }
        return;
    }
    match ask {
        Ask::Host => {
            let name = typed(&fields, f.party);
            session.lobby.name = if name.is_empty() {
                default_party_name(&session.name, class.name)
            } else {
                name
            };
            session.lobby.password = typed(&fields, f.host_pass);
            out.write(CoopCommand::Host);
        }
        Ask::Join => {
            let addr = typed(&fields, f.addr);
            if addr.is_empty() {
                session.status = "Type the host's address first.".to_string();
                focus.0 = Some(f.addr);
                return;
            }
            view.joining = addr.clone();
            out.write(CoopCommand::Join {
                addr,
                password: typed(&fields, f.join_pass),
            });
        }
        Ask::Row(i) => {
            let Some(game) = lan.games.get(i).filter(|g| joinable(g)).cloned() else {
                return;
            };
            let addr = game.addr.to_string();
            view.tab = Tab::Join;
            if game.locked {
                // The password is the one thing the list cannot send: fill the address in and ask.
                if let Ok(mut field) = fields.get_mut(f.addr) {
                    field.value = addr;
                }
                if let Ok(mut field) = fields.get_mut(f.join_pass) {
                    field.value.clear();
                }
                focus.0 = Some(f.join_pass);
                session.status = format!("{} needs a password.", party_line(&game).0);
            } else {
                view.joining = addr.clone();
                out.write(CoopCommand::Join {
                    addr,
                    password: String::new(),
                });
            }
        }
    }
}

/// Put a class's icon in `image`, tinted; hidden while the class or the art is missing.
fn set_class_icon(image: &mut ImageNode, art: &GlueArt, class_id: Option<u8>, tint: Color) {
    let face = class_id.and_then(|id| {
        let (sheet, size) = art.classes.as_ref()?;
        class_tc(id).map(|tc| (sheet.clone(), tc_rect(*size, tc)))
    });
    let Some((sheet, rect)) = face else {
        if image.color != Color::NONE {
            image.color = Color::NONE;
        }
        return;
    };
    if image.image != sheet {
        image.image = sheet;
    }
    if image.rect != Some(rect) {
        image.rect = Some(rect);
    }
    if image.color != tint {
        image.color = tint;
    }
}

/// Set a text and its colour, writing only on change.
fn set_text(text: &mut Text, color: &mut TextColor, want: &str, tint: Color) {
    if text.0 != want {
        text.0 = want.to_string();
    }
    if color.0 != tint {
        color.0 = tint;
    }
}

/// Show or hide a node, writing only on change.
fn set_shown(node: &mut Node, shown: bool) {
    let want = if shown { Display::Flex } else { Display::None };
    if node.display != want {
        node.display = want;
    }
}

/// While the LAN tab is in front: the rows from the parties found.
#[allow(clippy::too_many_arguments)]
fn refresh_lan(
    ui: Res<CoopUi>,
    view: Res<LobbyView>,
    lan: Res<LanGames>,
    art: Res<GlueArt>,
    hovered: Query<(&Interaction, &LanRow)>,
    mut texts: Query<(&Txt, &mut Text, &mut TextColor), Without<KitCaption>>,
    mut shows: Query<(&Show, &mut Node)>,
    mut paints: Query<(&Paint, &mut BackgroundColor)>,
    mut icons: Query<(&Icon, &mut ImageNode)>,
) {
    if !ui.open || view.tab != Tab::Lan {
        return;
    }
    let games = &lan.games[..lan.games.len().min(MAX_ROWS)];
    let mut hot = [false; MAX_ROWS];
    for (interaction, &LanRow(i)) in &hovered {
        if let Some(h) = hot.get_mut(i) {
            *h = *interaction != Interaction::None;
        }
    }

    for (show, mut node) in &mut shows {
        let shown = match *show {
            Show::Row(i) => i < games.len(),
            Show::Lock(i) => games.get(i).is_some_and(|g| g.locked),
            Show::Dot(i, j) => games
                .get(i)
                .is_some_and(|g| j < usize::from(g.max_players.min(MAX_DOTS))),
            Show::NoGames => games.is_empty(),
            _ => continue,
        };
        set_shown(&mut node, shown);
    }
    for (txt, mut text, mut color) in &mut texts {
        let (i, base) = match *txt {
            Txt::RowName(i) => (i, kit::CREAM),
            Txt::RowSub(i) | Txt::RowCount(i) => (i, kit::MUTED),
            Txt::RowZone(i) => (i, kit::CREAM),
            _ => continue,
        };
        let Some(game) = games.get(i) else { continue };
        let want = match *txt {
            Txt::RowName(_) => party_line(game).0,
            Txt::RowSub(_) => party_line(game).1,
            Txt::RowZone(_) => data::MAPS.get(game.zone).map_or("", |m| m.name).to_string(),
            _ => format!("{}/{}", game.players, game.max_players),
        };
        let tint = if joinable(game) { base } else { kit::DISABLED };
        set_text(&mut text, &mut color, &want, tint);
    }
    for (paint, mut fill) in &mut paints {
        let want = match *paint {
            Paint::Row(i) => match games.get(i) {
                Some(g) if !joinable(g) => ROW_FILL_FULL,
                Some(_) if hot[i] => ROW_FILL_HOT,
                _ => kit::INSET_FILL,
            },
            Paint::Dot(i, j) => match games.get(i) {
                Some(g) if dots(g.players, g.max_players).get(j) == Some(&true) => {
                    if joinable(g) {
                        kit::TITLE
                    } else {
                        kit::DISABLED
                    }
                }
                _ => Color::NONE,
            },
        };
        if fill.0 != want {
            fill.0 = want;
        }
    }
    for (icon, mut image) in &mut icons {
        let Icon::Row(i) = *icon else { continue };
        let Some(game) = games.get(i) else { continue };
        let tint = if joinable(game) { Color::WHITE } else { ICON_DIM };
        set_class_icon(&mut image, &art, Some(game.class_id), tint);
    }
}

/// While open: the tabs, the Host and Join bodies and the status line from the session.
#[allow(clippy::too_many_arguments)]
fn refresh_state(
    ui: Res<CoopUi>,
    view: Res<LobbyView>,
    session: Res<Session>,
    selection: Res<Selection>,
    profile: Option<Res<Profile>>,
    art: Res<GlueArt>,
    mut texts: Query<(&Txt, &mut Text, &mut TextColor), Without<KitCaption>>,
    mut shows: Query<(&Show, &mut Node)>,
    mut buttons: Query<(&LobbyAction, &mut KitButton, &Children)>,
    mut captions: Query<&mut Text, With<KitCaption>>,
    mut icons: Query<(&Icon, &mut ImageNode)>,
    mut fields: Query<&mut TextField>,
) {
    if !ui.open {
        return;
    }
    let class = selection.class_def();
    let port = match session.link {
        Link::Hosting { port } => Some(port),
        _ => None,
    };
    let hosting = port.is_some();
    let linked = matches!(session.link, Link::Joining | Link::Joined);
    let who = session.name.trim();
    let hero = class.map(|c| hero_line(who, c.name));
    let place = {
        let zone = data::MAPS.get(selection.map).map_or("", |m| m.name);
        let difficulty = profile.as_ref().map_or("Normal", |p| p.difficulty.label());
        format!("{zone} - {difficulty}")
    };
    let seats = seat_lines(hero.as_deref().unwrap_or(who), &session.guests, session.lobby.max_players);

    for (show, mut node) in &mut shows {
        let shown = match *show {
            Show::Body(tab) => view.tab == tab,
            Show::HostingBlock | Show::Veil => hosting,
            Show::Seat(i) => i < seats.len(),
            Show::JoinForm => !linked,
            Show::JoinStatus => linked,
            _ => continue,
        };
        set_shown(&mut node, shown);
    }
    for (txt, mut text, mut color) in &mut texts {
        let (want, tint) = match *txt {
            Txt::Hero => match &hero {
                Some(h) => (h.clone(), kit::CREAM),
                None => ("Pick a class on the hub first".to_string(), kit::MUTED),
            },
            Txt::Where if class.is_some() => (place.clone(), kit::MUTED),
            Txt::Where => (String::new(), kit::MUTED),
            Txt::Hosting => (
                port.map(|p| hosting_line(view.ip, p)).unwrap_or_default(),
                kit::TITLE,
            ),
            Txt::Seat(i) => match seats.get(i) {
                Some((line, true)) => (line.clone(), kit::CREAM),
                Some((line, false)) => (line.clone(), kit::MUTED),
                None => continue,
            },
            Txt::JoinNote if hosting => (
                "You are hosting a party. Close it on the Host tab to join another.".to_string(),
                kit::WARN,
            ),
            Txt::JoinNote => (
                "You play the hero from your hub pick, in the host's zone.".to_string(),
                kit::MUTED,
            ),
            Txt::JoinLine => (
                join_line(session.link, session.player, &view.joining),
                kit::TITLE,
            ),
            Txt::Status => (session.status.clone(), kit::CREAM),
            _ => continue,
        };
        set_text(&mut text, &mut color, &want, tint);
    }
    for (action, mut button, children) in &mut buttons {
        let (selected, disabled) = match *action {
            LobbyAction::Tab(tab) => (view.tab == tab, false),
            LobbyAction::Public(public) => (session.lobby.public == public, false),
            LobbyAction::Size(n) => (session.lobby.max_players == n, false),
            LobbyAction::Party => (false, !hosting && (class.is_none() || linked)),
            LobbyAction::Join => (false, hosting || class.is_none()),
            _ => continue,
        };
        if button.selected != selected {
            button.selected = selected;
        }
        if button.disabled != disabled {
            button.disabled = disabled;
        }
        if *action == LobbyAction::Party {
            let want = if hosting { "Close party" } else { "Open party" };
            for child in children.iter() {
                if let Ok(mut caption) = captions.get_mut(child) {
                    if caption.0 != want {
                        caption.0 = want.to_string();
                    }
                }
            }
        }
    }
    for (icon, mut image) in &mut icons {
        if *icon == Icon::Host {
            set_class_icon(&mut image, &art, class.map(|c| c.id), Color::WHITE);
        }
    }
    // The party name field hints at the name an empty field becomes.
    if let (Some(f), Some(class)) = (view.fields, class) {
        if let Ok(mut field) = fields.get_mut(f.party) {
            let want = default_party_name(who, class.name);
            if field.placeholder != want {
                field.placeholder = want;
            }
        }
    }
}

/// The wheel scrolls the party list while the mouse is over it.
fn scroll(
    ui: Res<CoopUi>,
    view: Res<LobbyView>,
    mut wheel: MessageReader<MouseWheel>,
    mut areas: Query<(&RelativeCursorPosition, &mut ScrollPosition), With<LobbyScroll>>,
) {
    let mut dy = 0.0;
    for w in wheel.read() {
        dy += match w.unit {
            MouseScrollUnit::Line => w.y * WHEEL_LINE,
            MouseScrollUnit::Pixel => w.y,
        };
    }
    if !ui.open || view.tab != Tab::Lan || dy == 0.0 {
        return;
    }
    for (cursor, mut pos) in &mut areas {
        if cursor.cursor_over() {
            pos.y = (pos.y - dy).max(0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coop::wire::Hello;

    fn game(players: u8, max: u8) -> LanGame {
        LanGame {
            addr: "192.168.1.20:37373".parse().unwrap(),
            lobby: "Murloc hunt".into(),
            host: "Thrall".into(),
            class_id: 7,
            zone: 0,
            players,
            max_players: max,
            locked: false,
            seen: 0.0,
        }
    }

    fn guest(player: u8, name: &str, class_id: u8) -> Guest {
        Guest {
            client: Entity::PLACEHOLDER,
            player,
            hello: Hello {
                name: name.into(),
                class_id,
                display: 0,
                race: 1,
                sex: 0,
                password: String::new(),
            },
        }
    }

    /// Two queries on one component that overlap panic when the schedule first initialises the
    /// systems, which would be at the game's start; this finds it without a window.
    #[test]
    fn the_systems_do_not_conflict() {
        let mut world = World::new();
        let mut schedule = Schedule::default();
        schedule.add_systems((build, show_panel, clicks, refresh_lan, refresh_state, scroll));
        schedule.initialize(&mut world).expect("the systems fit together");
    }

    #[test]
    fn classes_are_named_by_id() {
        assert_eq!(class_name(8), "Mage");
        assert_eq!(class_name(7), "Shaman");
        assert_eq!(class_name(200), "Adventurer");
    }

    #[test]
    fn the_default_party_name_uses_the_hero() {
        assert_eq!(default_party_name("Thrall", "Shaman"), "Thrall's party");
        assert_eq!(default_party_name("  ", "Shaman"), "Shaman's party");
        assert_eq!(default_party_name("", "Mage"), "Mage's party");
    }

    #[test]
    fn a_hero_line_skips_a_name_that_is_the_class() {
        assert_eq!(hero_line("Thrall", "Shaman"), "Thrall - Shaman");
        assert_eq!(hero_line("", "Shaman"), "Shaman");
        assert_eq!(hero_line("Shaman", "Shaman"), "Shaman");
    }

    #[test]
    fn a_row_has_the_party_and_its_host() {
        assert_eq!(
            party_line(&game(1, 4)),
            ("Murloc hunt".to_string(), "Thrall - Shaman".to_string())
        );
        let mut unnamed = game(1, 4);
        unnamed.lobby = " ".into();
        assert_eq!(party_line(&unnamed).0, "Thrall's party");
    }

    #[test]
    fn a_full_party_cannot_be_joined() {
        assert!(joinable(&game(1, 4)));
        assert!(joinable(&game(3, 4)));
        assert!(!joinable(&game(4, 4)));
        assert!(!joinable(&game(2, 2)));
        assert!(!joinable(&game(5, 4)), "more than the party takes is full too");
    }

    #[test]
    fn dots_fill_from_the_left() {
        assert_eq!(dots(2, 4), vec![true, true, false, false]);
        assert_eq!(dots(1, 2), vec![true, false]);
        assert_eq!(dots(0, 3), vec![false; 3]);
        assert_eq!(dots(9, 3), vec![true; 3], "never more filled than seats");
        assert_eq!(dots(2, 9).len(), usize::from(MAX_DOTS));
    }

    #[test]
    fn hosting_shows_the_address_friends_type() {
        assert_eq!(
            hosting_line(Some("192.168.1.20".parse().unwrap()), 37373),
            "Hosting on 192.168.1.20:37373"
        );
        assert_eq!(hosting_line(None, 37373), "Hosting on port 37373");
        assert_eq!(
            hosting_line(Some("fe80::1".parse().unwrap()), 5000),
            "Hosting on [fe80::1]:5000"
        );
    }

    #[test]
    fn seats_list_the_host_the_guests_and_the_free_ones() {
        let seats = seat_lines("Thrall - Shaman", &[guest(1, "Jaina", 8)], 3);
        assert_eq!(
            seats,
            vec![
                ("Thrall - Shaman".to_string(), true),
                ("Jaina - Mage".to_string(), true),
                ("Waiting for a player...".to_string(), false),
            ]
        );
        assert_eq!(seat_lines("Host", &[], 2).len(), 2);
        assert_eq!(
            seat_lines("Host", &[guest(1, "A", 8), guest(2, "B", 8), guest(3, "C", 8)], 2).len(),
            4,
            "guests already seated are all listed"
        );
    }

    #[test]
    fn the_join_line_follows_the_link() {
        assert_eq!(
            join_line(Link::Joining, None, "192.168.1.20"),
            "Joining 192.168.1.20..."
        );
        assert_eq!(join_line(Link::Joining, None, ""), "Joining the party...");
        assert_eq!(
            join_line(Link::Joined, Some(1), "x"),
            "In the party as player 2"
        );
        assert_eq!(join_line(Link::Joined, None, "x"), "In the party");
        assert_eq!(join_line(Link::Off, None, "x"), "");
    }
}

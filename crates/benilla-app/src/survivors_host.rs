//! The engine surface the Warcraft Survivors crate (`survivors-app`) builds on. Fork-owned: every line
//! here is a `pub use` of an item that is `pub` in its home module, so upstream files change by a
//! visibility keyword only. The module layout mirrors this crate's, so a `crate::glue::art::GlueArt` in
//! the survivors tree is `benilla_app::survivors_host::glue::art::GlueArt` there.

pub mod area {
    pub use crate::area::AreaTableRes;
}

pub mod char_select {
    pub use crate::char_select::{race_name, wow_font};
}

pub mod combat_text {
    pub use crate::combat_text::CombatTextSpawn;
}

pub mod creature_anim {
    pub use crate::creature_anim::select::{move_flags, MovementState};
    pub use crate::creature_anim::{
        CastEvent, CastEventKind, PlaySeq, SpellGoTargets, SwingMessage,
    };
}

pub mod cvars {
    pub use crate::cvars::{Cvars, SetOutcome};
}

pub mod entities {
    pub use crate::entities::equipment::BoneAttach;
    pub use crate::entities::{overhead_anchor, Creatures, OverheadFallback};

    pub mod mount {
        pub use crate::entities::mount::MountChild;
    }
}

pub mod glue {
    pub use crate::glue::{GlueClicks, GlueVisuals};

    pub mod add_material {
        pub use crate::glue::add_material::AddUiMaterial;
    }

    pub mod art {
        pub use crate::glue::art::{
            class_tc, race_tc, tc_rect, GlueArt, DIM, GOLD, NAME_EDGE,
        };
    }

    pub mod backdrop {
        pub use crate::glue::backdrop::{
            backdrop_border, backdrop_edges, tiled_bg_node, BackdropEdges,
        };
    }

    pub mod widgets {
        pub use crate::glue::widgets::{
            abs, glue_button, icon_button, outlined_text, overlay, GlueBtnKind, GlueCaption,
            GlueDisabled, GlueText, Hilight, HoverLabel, LockHighlight,
        };
    }
}

pub mod loading_screen {
    pub use crate::loading_screen::LoadingScreen;
}

pub mod local_state {
    pub use crate::local_state::{home, read_to_string, write_atomic};
}

pub mod minimap {
    pub mod blips {
        pub use crate::minimap::blips::PLAYER_ARROW_TEXTURE;
    }
}

pub mod net {
    pub use crate::net::motion::facing::ScriptedFacing;
    pub use crate::net::{FieldChanged, Guid, NetEntity, ObjectStore};
}

pub mod player {
    pub use crate::player::state::Player;
}

pub mod run_mode {
    pub use crate::run_mode::CaptureMode;
}

pub mod sound {
    pub use crate::sound::glue::GlueSound;
}

pub mod textinput {
    pub use crate::textinput::clipboard::{wayland_display, HostClipboard};
}

pub mod ui_action {
    pub use crate::ui_action::Spells;
}

pub mod ui_pass {
    pub use crate::ui_pass::project_overlay;
}

pub mod video {
    pub use crate::video::{parse_resolution, windowed_env};
}

pub mod vplates {
    pub use crate::vplates::{
        con_color, gx_px, plate_basis, text_px, BAR_H, BAR_OFF_X, BAR_OFF_Y, BAR_W, CON_YELLOW,
        LEVEL_H, LEVEL_OFF_X, LEVEL_OFF_Y, PLATE_FRIENDLY, PLATE_H, PLATE_W,
    };
}

pub mod webenv {
    pub use crate::webenv::var;
}

pub mod world_backdrop {
    pub use crate::world_backdrop::RenderScale;
}

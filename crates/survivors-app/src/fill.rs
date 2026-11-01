//! The overlays' big soft gradients as one dithered material (`shaders/survivors_fill.wgsl`).
//! Bevy's `BackgroundGradient` writes straight to the 8-bit target, so long fades across dark
//! values band; this mixes the same way (Oklab) and adds sub-byte noise before the write.

use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy::ui_render::ui_material::UiMaterial;

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub(super) struct FillMaterial {
    #[uniform(0)]
    fill: FillUniform,
}

#[derive(Clone, Copy, ShaderType)]
struct FillUniform {
    a: Vec4,
    b: Vec4,
    c: Vec4,
    glow: Vec4,
    glow_at: Vec4,
    shape: Vec4,
}

impl UiMaterial for FillMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://benilla_app/shaders/survivors_fill.wgsl".into()
    }
}

/// A transparent 1×1 node with this material that is always drawn, so its one pipeline compiles
/// behind the loading cover instead of live at the first level-up, a visible stall on macOS
/// (`benilla_app::pipe_warm`'s tripwire logged it as `pipeline compiled LIVE`).
pub(super) fn spawn_warm(commands: &mut Commands, fills: &mut Assets<FillMaterial>) {
    commands.spawn((
        Name::new("survivors fill warm"),
        Node {
            position_type: PositionType::Absolute,
            width: Val::Px(1.0),
            height: Val::Px(1.0),
            ..default()
        },
        MaterialNode(fills.add(FillMaterial::clear())),
        GlobalZIndex(-1),
        bevy::ui::FocusPolicy::Pass,
    ));
}

fn lin(c: Color) -> Vec4 {
    c.to_linear().to_vec4()
}

impl FillMaterial {
    /// From `centre` at the middle to `edge` at the farthest corner.
    pub(super) fn radial(centre: Color, edge: Color) -> Self {
        Self {
            fill: FillUniform {
                a: lin(centre),
                b: Vec4::ZERO,
                c: lin(edge),
                glow: Vec4::ZERO,
                glow_at: Vec4::ZERO,
                shape: Vec4::new(0.0, 1.0, 0.5, 0.0),
            },
        }
    }

    /// Draws nothing: every colour is fully transparent. For [`spawn_warm`].
    fn clear() -> Self {
        Self::radial(Color::NONE, Color::NONE)
    }

    /// Lays an elliptical light over the fill: `color` (its alpha is the peak) at `at` in UV,
    /// fading to nothing at `radii`, fractions of the node's height.
    pub(super) fn with_glow(mut self, color: Color, at: Vec2, radii: Vec2) -> Self {
        self.fill.glow = lin(color);
        self.fill.glow_at = Vec4::new(at.x, at.y, radii.x, radii.y);
        self
    }
}

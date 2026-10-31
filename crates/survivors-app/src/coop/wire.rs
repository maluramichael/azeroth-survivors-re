//! What crosses the wire: the components every hero replicates with, and the messages between a
//! guest and its host. Registered in one order on both ends ([`protocol`]); replicon checks the
//! two match at the handshake.

use bevy::prelude::*;
use bevy_replicon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::data::Weapon;
use crate::units::UnitSpec;

// Replicated components ------------------------------------------------------------------------

/// What a unit's body is built from: [`UnitSpec`] without the serial, which each machine picks.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub(super) struct Look {
    pub display: u32,
    pub scale: f32,
    pub level: u32,
    pub faction: u32,
    pub bytes0: u32,
    pub weapons: Vec<Weapon>,
    pub sheath: u8,
}

impl Look {
    pub fn from_spec(spec: &UnitSpec) -> Self {
        Self {
            display: spec.display,
            scale: spec.scale,
            level: spec.level,
            faction: spec.faction,
            bytes0: spec.bytes0,
            weapons: spec.weapons.to_vec(),
            sheath: spec.sheath,
        }
    }

    /// The spec to build this look's body under `serial`.
    pub fn spec(&self, serial: u64) -> UnitSpec<'_> {
        UnitSpec {
            serial,
            display: self.display,
            scale: self.scale,
            level: self.level,
            faction: self.faction,
            bytes0: self.bytes0,
            weapons: &self.weapons,
            sheath: self.sheath,
        }
    }
}

/// Where a unit stands, quantised: centimetres, a 256-step facing, and the walk the animation
/// shows (pace in cm/s and the movement flags). From deanmathieson's fork (`coop/wire.rs`).
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct NetPos {
    pub at: [i32; 3],
    pub yaw: u8,
    pub pace: u16,
    pub flags: u32,
}

impl NetPos {
    pub fn new(pos: Vec3, yaw: f32, pace: f32, flags: u32) -> Self {
        let cm = |v: f32| (v * 100.0).round() as i32;
        let turn = yaw.rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU;
        Self {
            at: [cm(pos.x), cm(pos.y), cm(pos.z)],
            yaw: ((turn * 256.0).round() as u32 % 256) as u8,
            pace: (pace.max(0.0) * 100.0).round().min(f32::from(u16::MAX)) as u16,
            flags,
        }
    }

    pub fn pos(&self) -> Vec3 {
        Vec3::new(
            self.at[0] as f32 / 100.0,
            self.at[1] as f32 / 100.0,
            self.at[2] as f32 / 100.0,
        )
    }

    /// The facing in radians, in `[0, τ)`.
    pub fn yaw(&self) -> f32 {
        f32::from(self.yaw) / 256.0 * std::f32::consts::TAU
    }

    pub fn pace(&self) -> f32 {
        f32::from(self.pace) / 100.0
    }
}

/// Who a hero is: its player (0 is the host), name and class.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub(super) struct NetHero {
    pub player: u8,
    pub name: String,
    pub class_id: u8,
}

// Guest to host --------------------------------------------------------------------------------

/// A guest's first word: who it is and what its hero looks like.
#[derive(Message, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub(super) struct Hello {
    pub name: String,
    pub class_id: u8,
    pub display: u32,
    pub race: u8,
    pub sex: u8,
}

/// Where the guest's own hero is, 20 times a second.
#[derive(Message, Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub(super) struct MoveInput {
    pub at: [f32; 3],
    pub yaw: f32,
    pub pace: f32,
    pub flags: u32,
}

// Host to guest --------------------------------------------------------------------------------

/// The guest has a seat: its player number.
#[derive(Message, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Welcome {
    pub player: u8,
}

/// The guest is turned away, and why.
#[derive(Message, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub(super) struct Refused {
    pub reason: String,
}

/// The host's run began (or is under way): the guest enters the same battleground.
#[derive(Message, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct StartRun {
    pub map: usize,
}

/// The host is back at the menu: so is the guest.
#[derive(Message, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct EndRun;

/// The replication rules and messages, in the one order both ends register them.
pub(super) fn protocol(app: &mut App) {
    app.replicate::<Look>()
        .replicate::<NetPos>()
        .replicate::<NetHero>()
        .add_client_message::<Hello>(Channel::Ordered)
        .add_client_message::<MoveInput>(Channel::Unreliable)
        .add_server_message::<Welcome>(Channel::Ordered)
        .add_server_message::<Refused>(Channel::Ordered)
        .make_message_independent::<Refused>()
        .add_server_message::<StartRun>(Channel::Ordered)
        .add_server_message::<EndRun>(Channel::Ordered);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn net_pos_round_trips_to_a_centimetre() {
        let p = NetPos::new(Vec3::new(12.345, -3.21, 700.009), 1.0, 7.0, 3);
        assert!((p.pos() - Vec3::new(12.345, -3.21, 700.009)).abs().max_element() <= 0.006);
        assert!((p.yaw() - 1.0).abs() < std::f32::consts::TAU / 256.0);
        assert!((p.pace() - 7.0).abs() < 0.01);
        assert_eq!(p.flags, 3);
    }

    #[test]
    fn a_look_rebuilds_the_spec_it_came_from() {
        let weapons = [(0u16, 1927u32, 2u8, 10u8, 2u8, 17u8, 2u8), (1, 2001, 4, 6, 1, 14, 3)];
        let spec = UnitSpec {
            serial: 7,
            display: 1234,
            scale: 1.0,
            level: 60,
            faction: 35,
            bytes0: 0x0109_0105,
            weapons: &weapons,
            sheath: 1,
        };
        let look = Look::from_spec(&spec);
        let back = look.spec(7);
        assert_eq!(back.serial, 7);
        assert_eq!(back.display, spec.display);
        assert_eq!(back.scale, spec.scale);
        assert_eq!(back.level, spec.level);
        assert_eq!(back.faction, spec.faction);
        assert_eq!(back.bytes0, spec.bytes0);
        assert_eq!(back.weapons, spec.weapons);
        assert_eq!(back.sheath, spec.sheath);
    }
}

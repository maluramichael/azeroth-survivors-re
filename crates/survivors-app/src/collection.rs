//! The collection book: every item template ([`super::items::table::TEMPLATES`]) by slot and
//! quality, the ones ever picked up shown, the rest as question marks, with a detail pane for the
//! clicked piece. Opened from the hub's Collection tab; what was found lives in
//! [`super::profile::Profile::collection`].

use bevy::prelude::*;

/// Whether the collection window is open; the hub's tab sets it.
#[derive(Resource, Default)]
pub(crate) struct CollectionUi {
    pub(crate) open: bool,
}

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<CollectionUi>();
}

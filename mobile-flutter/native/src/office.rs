//! Reports an office room's state to Flutter: joined or not, who else is there, and whether the
//! realtime connection gave up.

use bevy::prelude::*;
use officexr_client::{Joined, Peers, RoomClosed};

use crate::events::{EventSender, FpsCounter, HostEvent};

pub struct OfficeReportPlugin;

impl Plugin for OfficeReportPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (report_closed, report_room));
    }
}

fn report_closed(mut closed: MessageReader<RoomClosed>, events: Res<EventSender>) {
    for RoomClosed(reason) in closed.read() {
        events.send(HostEvent::Closed {
            reason: reason.clone(),
        });
    }
}

fn report_room(
    time: Res<Time>,
    mut counter: Local<FpsCounter>,
    mut joined_once: Local<bool>,
    mut joined: MessageReader<Joined>,
    peers: Res<Peers>,
    events: Res<EventSender>,
) {
    *joined_once |= joined.read().count() > 0;
    let Some(fps) = counter.tick(time.delta_secs()) else {
        return;
    };
    events.send(HostEvent::Room {
        fps,
        joined: *joined_once,
        peers: peers.len(),
    });
}

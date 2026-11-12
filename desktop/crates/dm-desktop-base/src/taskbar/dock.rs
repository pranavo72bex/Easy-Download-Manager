// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::Outside;
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSDockTile, NSImageView, NSProgressIndicator, NSProgressIndicatorStyle,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

pub(super) struct Tile {
    tile: Retained<NSDockTile>,
    picture: Retained<NSImageView>,
    bar: Retained<NSProgressIndicator>,
}

impl Tile {
    /// None off the main thread, which AppKit's objects belong to, or
    /// where the program has no picture yet.
    pub(super) fn new() -> Option<Tile> {
        let mtm = MainThreadMarker::new()?;
        let app = NSApplication::sharedApplication(mtm);
        let tile = app.dockTile();
        let icon = app.applicationIconImage()?;
        let size = tile.size();
        let picture = NSImageView::imageViewWithImage(&icon, mtm);
        picture.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), size));
        // A bar a tenth of the tile tall, across its foot, as Finder's
        // own copy shows one.
        let frame = NSRect::new(
            NSPoint::new(size.width * 0.08, size.height * 0.06),
            NSSize::new(size.width * 0.84, (size.height * 0.1).max(8.0)),
        );
        let bar = NSProgressIndicator::initWithFrame(NSProgressIndicator::alloc(mtm), frame);
        bar.setStyle(NSProgressIndicatorStyle::Bar);
        bar.setIndeterminate(false);
        bar.setMinValue(0.0);
        bar.setMaxValue(100.0);
        bar.setHidden(true);
        picture.addSubview(&bar);
        Some(Tile { tile, picture, bar })
    }

    pub(super) fn show(&self, now: Outside) {
        match now.percent {
            Some(percent) => {
                self.bar.setDoubleValue(f64::from(percent));
                self.bar.setHidden(false);
                self.tile.setContentView(Some(&self.picture));
            }
            None => {
                self.bar.setHidden(true);
                // The Dock draws dm's own picture again.
                self.tile.setContentView(None);
            }
        }
        let badge = (now.running > 0).then(|| NSString::from_str(&now.running.to_string()));
        self.tile.setBadgeLabel(badge.as_deref());
        self.tile.display();
    }
}

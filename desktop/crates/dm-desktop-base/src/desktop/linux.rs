// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Linux: the file manager and the portal, asked on the session bus.

/// Shows `path` selected in its folder through the freedesktop file
/// manager interface, which the usual file managers answer (and are
/// started for). False where none is there to ask.
pub(super) fn show_item(path: &str) -> bool {
    const NAME: &str = "org.freedesktop.FileManager1";
    let Ok(connection) = zbus::blocking::Connection::session() else {
        return false;
    };
    let there = zbus::blocking::fdo::DBusProxy::new(&connection).is_ok_and(|bus| {
        let Ok(name) = zbus::names::BusName::try_from(NAME) else {
            return false;
        };
        bus.name_has_owner(name).unwrap_or(false)
            || bus
                .list_activatable_names()
                .is_ok_and(|names| names.iter().any(|n| n.as_str() == NAME))
    });
    if !there {
        return false;
    }
    let proxy = zbus::blocking::proxy::Builder::<zbus::blocking::Proxy>::new(&connection)
        .destination(NAME)
        .and_then(|b| b.path("/org/freedesktop/FileManager1"))
        .and_then(|b| b.interface(NAME))
        .map(|b| b.cache_properties(zbus::proxy::CacheProperties::No))
        .and_then(|b| b.build());
    let Ok(proxy) = proxy else {
        return false;
    };
    proxy
        .call::<_, _, ()>("ShowItems", &(vec![crate::format::file_uri(path)], ""))
        .is_ok()
}

/// Whether the desktop portal offers its file chooser on the session bus.
pub(super) fn file_chooser_portal() -> bool {
    let Ok(connection) = zbus::blocking::Connection::session() else {
        return false;
    };
    let proxy = zbus::blocking::proxy::Builder::<zbus::blocking::Proxy>::new(&connection)
        .destination("org.freedesktop.portal.Desktop")
        .and_then(|b| b.path("/org/freedesktop/portal/desktop"))
        .and_then(|b| b.interface("org.freedesktop.portal.FileChooser"))
        .map(|b| b.cache_properties(zbus::proxy::CacheProperties::No))
        .and_then(|b| b.build());
    proxy.is_ok_and(|p| p.get_property::<u32>("version").is_ok())
}

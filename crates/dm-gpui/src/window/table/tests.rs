// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

#[test]
fn the_list_is_told_as_a_table_of_headings_and_cells() {
    use crate::window::a11y::node_of;
    // The header's row and three of downloads, in four columns.
    let table = node_of(&as_table(div().id("table"), 3, 4));
    assert_eq!(table.role(), Role::Table);
    assert_eq!(table.label(), Some("Downloads"));
    assert_eq!(table.row_count(), Some(4));
    assert_eq!(table.column_count(), Some(4));

    let heading = node_of(&told_heading(div().id("size"), Column::Size, 2, Some(true)));
    assert_eq!(heading.role(), Role::ColumnHeader);
    assert_eq!(heading.label(), Some(Column::Size.title()));
    assert_eq!(heading.column_index(), Some(2));

    let cell = node_of(&told_cell(div().id(("size", 7usize)), 2, "1.5 MB"));
    assert_eq!(cell.role(), Role::Cell);
    assert_eq!(cell.label(), Some("1.5 MB"));
    assert_eq!(cell.column_index(), Some(2));
}

#[test]
fn a_heading_dropped_on_another_takes_its_place() {
    use Column::{Name, Queue, Size, Status};
    let order = vec![Name, Queue, Size, Status];
    // From the left: after the one dropped on.
    assert_eq!(
        moved_onto(order.clone(), Queue, Status),
        Some(vec![Name, Size, Status, Queue])
    );
    // From the right: before it.
    assert_eq!(
        moved_onto(order.clone(), Status, Queue),
        Some(vec![Name, Status, Queue, Size])
    );
    assert_eq!(moved_onto(order, Column::Rate, Queue), None);
}

/// A column a person left alone is as wide as its heading and a long figure, never under
/// its default; Queue, where no row shown waits in one, only as wide as its heading; Status
/// keeps its own, as it grows.
#[test]
fn a_column_is_as_wide_as_its_heading_and_figures() {
    use crate::widgets::native::{small_width, text_width};
    for column in [
        Column::Size,
        Column::TimeLeft,
        Column::Rate,
        Column::LastTry,
    ] {
        let own = own_width(column, true, true).unwrap();
        assert!(own >= width(column).unwrap(), "{column:?}");
        assert!(own >= small_width(column.title()) + 16.0, "{column:?}");
    }
    let size = own_width(Column::Size, true, true).unwrap();
    assert!(size >= text_width(&format::bytes(1_073_217_536)) + 16.0);
    let empty_queue = own_width(Column::Queue, false, true).unwrap();
    assert!(empty_queue < width(Column::Queue).unwrap(), "{empty_queue}");
    assert!(empty_queue >= small_width(Column::Queue.title()) + 16.0);
    assert_eq!(own_width(Column::Status, true, true), width(Column::Status));
    assert_eq!(own_width(Column::Name, true, true), None);
}

/// Where the list has no room for every heading whole, a column is as wide as its figures need,
/// never under its default: its heading is cut (whole on hover) rather than the list scrolling
/// sideways at a new window's size.
#[test]
fn without_room_a_column_keeps_its_figures_and_gives_up_its_heading() {
    use crate::widgets::native::text_width;
    let rate = own_width(Column::Rate, true, false).unwrap();
    assert!(rate >= width(Column::Rate).unwrap());
    assert!(rate >= text_width(&format::rate(1_073_217_536.0)) + 16.0);
    assert!(rate <= own_width(Column::Rate, true, true).unwrap());
    let size = own_width(Column::Size, true, false).unwrap();
    assert!(size >= text_width(&format::bytes(1_073_217_536)) + 16.0);
    // As wide as the widest size under a thousand of a unit, as this computer counts.
    assert!(size >= text_width(&format::bytes(999_949_999)) + 16.0);
    assert!(size >= text_width(&format::bytes(419_400_000)) + 16.0);
}

/// Words are offered whole where they are wider than their room, as measured, not counted.
#[test]
fn cut_words_are_measured_against_their_room() {
    assert!(!cut("", 0.0));
    assert!(!cut("Complete", 200.0));
    assert!(cut("Complete", 10.0));
    let long = "This link no longer works";
    let wide = crate::widgets::native::text_width(long);
    assert!(cut(long, wide - 1.0) && !cut(long, wide + 1.0));
}

/// Room past every column's least goes two parts to the file name for one to Status, up to
/// twice Status's least; none to Status where it is hidden or given a width by hand. Status took
/// its whole least before the name had any of the room, and stood wider than a name it cut.
#[test]
fn status_grows_by_a_third_of_the_spare_room_and_the_name_by_the_rest() {
    let shown = [Column::Name, Column::Size, Column::Status];
    let none = Widths::new();
    let least = width(Column::Status).unwrap();
    assert_eq!(status_growth(&shown, &none, 90.0), 30.0);
    assert_eq!(status_growth(&shown, &none, 0.0), 0.0);
    // At most twice its least, however wide the window.
    assert_eq!(status_growth(&shown, &none, 10_000.0), least);
    assert_eq!(
        status_growth(&[Column::Name, Column::Size], &none, 90.0),
        0.0
    );
    let given = Widths::from([(Column::Status.key().to_string(), 150.0)]);
    assert_eq!(status_growth(&shown, &given, 90.0), 0.0);
}

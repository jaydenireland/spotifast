//! The command palette: Cmd+K (Ctrl+K elsewhere) opens a search box over
//! the page that finds songs, artists, albums and the listener's own
//! playlists, and plays the chosen one from the keyboard.

use std::time::Instant;

use egui::{
    Align, Align2, CornerRadius, Frame, Key, Layout, Margin, Modifiers, Rect, Sense, Stroke, Vec2,
    pos2, vec2,
};

use crate::api::models::pick_image;
use crate::app::App;
use crate::i18n::{Locale, gettext};
use crate::model::{Action, Loadable, PaletteEntry};
use crate::theme::{self, Icon};
use crate::util;

use super::widgets;

const ROW_HEIGHT: f32 = 48.0;
const COVER: f32 = 36.0;
const PADDING: f32 = 10.0;
const HEADING_HEIGHT: f32 = 26.0;

pub fn show(app: &mut App, ctx: &egui::Context) {
    let Some(palette) = &mut app.command_palette else {
        return;
    };
    // Up and Down move the choice and Enter plays it. They are taken before
    // the field sees them: the field would move its caret with Up and Down,
    // and it keeps the letters.
    let (down, up, enter) = ctx.input_mut(|input| {
        (
            input.count_and_consume_key(Modifiers::NONE, Key::ArrowDown),
            input.count_and_consume_key(Modifiers::NONE, Key::ArrowUp),
            input.consume_key(Modifiers::NONE, Key::Enter),
        )
    });
    let moved = down + up > 0;
    if moved {
        palette.move_choice(down as isize - up as isize);
    }
    let entries = palette.entries();
    if enter {
        if !entries.is_empty() {
            app.actions
                .push(Action::PlayCommandPaletteEntry(palette.highlighted));
        } else if palette.typed_at.is_some() {
            // Enter before the pause ends searches at once.
            palette.typed_at = Some(Instant::now() - crate::app::SEARCH_DEBOUNCE);
        }
    }
    let colours = app.palette;
    let locale = app.locale;
    let content = ctx.content_rect();
    let width = (content.width() - 48.0).clamp(280.0, 600.0);
    // Near the top, as palettes sit, with the page still in view under it.
    let top = (content.height() * 0.12).max(theme::top_bar_height() + 8.0);
    // The rows scroll once they would run past the bottom of the window.
    let room = (content.height() - top - 150.0).max(ROW_HEIGHT * 2.0);
    let id = egui::Id::new("command-palette");
    let field_id = id.with("field");
    let frame = Frame::new()
        .fill(colours.overlay)
        .stroke(Stroke::new(1.0, colours.outline))
        .corner_radius(CornerRadius::same(theme::RADIUS + 4))
        .inner_margin(Margin::same(PADDING as i8))
        .shadow(egui::epaint::Shadow {
            offset: [0, 10],
            blur: 40,
            spread: 0,
            color: colours.shadow,
        });
    let area = egui::Modal::default_area(id).anchor(Align2::CENTER_TOP, vec2(0.0, top));
    let searching = app.command_palette_searching();
    let response = egui::Modal::new(id)
        .area(area)
        .frame(frame)
        .backdrop_color(egui::Color32::from_black_alpha(if colours.dark {
            150
        } else {
            80
        }))
        .show(ctx, |ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            field(app, ui, field_id, width);
            let Some(palette) = &app.command_palette else {
                return;
            };
            let committed = palette.committed.clone();
            let highlighted = palette.highlighted;
            let state = match &palette.found {
                Loadable::Loaded(_) | Loadable::NotLoaded => None,
                Loadable::Loading => Some(Ok(())),
                Loadable::Failed(error) => Some(Err(error.clone())),
            };
            if !entries.is_empty() {
                ui.add_space(PADDING);
                crate::autoscroll::show(
                    ui,
                    egui::ScrollArea::vertical()
                        .max_height(room)
                        .auto_shrink([false, true]),
                    egui::Vec2b::new(false, true),
                    |ui| rows(app, ui, &entries, highlighted, moved),
                );
            } else if palette.found.get().is_some() {
                ui.add_space(PADDING);
                centered(ui, |ui| {
                    theme::subtle(
                        ui,
                        &colours,
                        // Translators: {query} is the text the user searched for.
                        &gettext(locale, "No results for “{query}”").replace("{query}", &committed),
                    );
                });
            }
            match state {
                Some(Ok(())) if entries.is_empty() => {
                    ui.add_space(PADDING);
                    centered(ui, |ui| {
                        theme::subtle(ui, &colours, &gettext(locale, "Searching…"));
                    });
                }
                Some(Err(error)) => {
                    ui.add_space(PADDING);
                    centered(ui, |ui| {
                        theme::text(ui, error, theme::regular(13.0), colours.danger);
                    });
                }
                _ => {}
            }
            ui.add_space(PADDING);
            footer(ui, &colours, locale, searching);
        });
    if response.backdrop_response.clicked() {
        app.actions.push(Action::CloseCommandPalette);
    }
}

/// The search field. It holds the keyboard while the palette is open, so
/// letters type into it instead of working as shortcuts.
fn field(app: &mut App, ui: &mut egui::Ui, id: egui::Id, width: f32) {
    let colours = app.palette;
    let locale = app.locale;
    let Some(palette) = &mut app.command_palette else {
        return;
    };
    let mut query = palette.query.clone();
    let response = widgets::search_field(
        ui,
        &colours,
        locale,
        id,
        &mut query,
        &gettext(locale, "Search for something to play"),
        width,
    );
    if palette.focus_requested || ui.memory(|memory| memory.focused().is_none()) {
        palette.focus_requested = false;
        response.request_focus();
    }
    if query != palette.query {
        palette.query = query;
        palette.typed_at = Some(Instant::now());
    }
}

/// The kinds of entry, each under its heading.
fn kind(entry: &PaletteEntry) -> u8 {
    match entry {
        PaletteEntry::Song(_) => 0,
        PaletteEntry::Artist(_) => 1,
        PaletteEntry::Album(_) => 2,
        PaletteEntry::Playlist(_) => 3,
    }
}

fn heading(locale: Locale, entry: &PaletteEntry) -> std::borrow::Cow<'static, str> {
    match entry {
        PaletteEntry::Song(_) => gettext(locale, "Songs"),
        PaletteEntry::Artist(_) => gettext(locale, "Artists"),
        PaletteEntry::Album(_) => gettext(locale, "Albums"),
        PaletteEntry::Playlist(_) => gettext(locale, "Your playlists"),
    }
}

/// One row per entry under its kind's heading: cover, name, a line about
/// it, and a song's length.
fn rows(
    app: &mut App,
    ui: &mut egui::Ui,
    entries: &[PaletteEntry],
    highlighted: usize,
    moved: bool,
) {
    let colours = app.palette;
    let locale = app.locale;
    let pointer_moved = ui.input(|input| input.pointer.delta() != Vec2::ZERO);
    let mut hovered_row = None;
    let mut last_kind = None;
    for (index, entry) in entries.iter().enumerate() {
        if last_kind != Some(kind(entry)) {
            last_kind = Some(kind(entry));
            let (rect, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), HEADING_HEIGHT), Sense::hover());
            if ui.is_rect_visible(rect) {
                let label = crate::bidi::layout_line(
                    ui.painter(),
                    heading(locale, entry).into_owned(),
                    theme::semibold(11.5),
                    colours.dim,
                );
                ui.painter().galley(
                    pos2(rect.left() + 8.0, rect.bottom() - 4.0 - label.size().y),
                    label,
                    colours.dim,
                );
            }
        }
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
        if response.hovered() && pointer_moved {
            hovered_row = Some(index);
        }
        if index == highlighted && moved {
            ui.scroll_to_rect(rect, None);
        }
        if !ui.is_rect_visible(rect) {
            continue;
        }
        if index == highlighted {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(6), colours.surface_hover);
        }
        let cover_rect = Rect::from_center_size(
            pos2(rect.left() + 6.0 + COVER / 2.0, rect.center().y),
            Vec2::splat(COVER),
        );
        let art = app.art_px(COVER);
        let (image, fallback, radius) = match entry {
            PaletteEntry::Song(track) => (track.image(art), Icon::Music, 4.0),
            PaletteEntry::Artist(artist) => {
                (pick_image(&artist.images, art), Icon::Mic, COVER / 2.0)
            }
            PaletteEntry::Album(album) => (pick_image(&album.images, art), Icon::Disc, 4.0),
            PaletteEntry::Playlist(playlist) => {
                (pick_image(&playlist.images, art), Icon::ListMusic, 4.0)
            }
        };
        widgets::paint_cover(
            ui,
            &colours,
            image,
            cover_rect,
            radius,
            fallback,
            Some(app.backend.art()),
        );
        let painter = ui.painter();
        let mut right = rect.right() - 12.0;
        if let PaletteEntry::Song(track) = entry {
            let duration = crate::bidi::layout_line(
                painter,
                util::format_duration_ms(track.duration_ms),
                theme::regular(12.5),
                colours.secondary,
            );
            let duration_pos = pos2(
                right - duration.size().x,
                rect.center().y - duration.size().y / 2.0,
            );
            painter.galley(duration_pos, duration, colours.secondary);
            right = duration_pos.x - 12.0;
        }
        // The name and, under it, a line about the entry, cut with an
        // ellipsis where the row runs out. Painted at measured positions:
        // a nested layout would pad the name row to a control's height and
        // push the second line into the row below.
        let left = cover_rect.right() + 12.0;
        let (name, subtitle, explicit) = match entry {
            PaletteEntry::Song(track) => {
                let mut subtitle = track.artist_names();
                if let Some(album) = track.album.as_ref().filter(|album| !album.name.is_empty()) {
                    if !subtitle.is_empty() {
                        subtitle.push_str(" · ");
                    }
                    subtitle.push_str(&album.name);
                }
                (track.name.as_str(), subtitle, track.explicit)
            }
            PaletteEntry::Artist(artist) => (
                artist.name.as_str(),
                gettext(locale, "Artist").into_owned(),
                false,
            ),
            PaletteEntry::Album(album) => {
                let mut subtitle = gettext(locale, "Album").into_owned();
                let artists = crate::api::models::join_names(
                    album.artists.iter().map(|artist| artist.name.as_str()),
                );
                if !artists.is_empty() {
                    subtitle.push_str(" · ");
                    subtitle.push_str(&artists);
                }
                (album.name.as_str(), subtitle, false)
            }
            PaletteEntry::Playlist(playlist) => (
                playlist.name.as_str(),
                gettext(locale, "Playlist").into_owned(),
                false,
            ),
        };
        let badge = if explicit { 21.0 } else { 0.0 };
        let name = crate::bidi::layout(
            painter,
            name,
            theme::semibold(14.0),
            colours.text,
            (right - left - badge).max(0.0),
            1,
            Some(crate::bidi::ELLIPSIS),
        );
        let subtitle = crate::bidi::layout(
            painter,
            &subtitle,
            theme::regular(12.5),
            colours.secondary,
            (right - left).max(0.0),
            1,
            Some(crate::bidi::ELLIPSIS),
        );
        let gap = 2.0;
        let block = name.size().y + gap + subtitle.size().y;
        let top = rect.center().y - block / 2.0;
        if explicit {
            let badge_rect = Rect::from_center_size(
                pos2(left + 7.5, top + name.size().y / 2.0),
                Vec2::splat(15.0),
            );
            painter.rect_filled(badge_rect, CornerRadius::same(2), colours.secondary);
            painter.text(
                badge_rect.center(),
                Align2::CENTER_CENTER,
                "E",
                theme::bold(9.5),
                colours.window,
            );
        }
        let name_rect =
            Rect::from_min_max(pos2(left + badge, top), pos2(right, top + name.size().y));
        painter.galley(
            crate::bidi::galley_pos(name_rect, &name),
            name,
            colours.text,
        );
        let subtitle_top = top + block - subtitle.size().y;
        let subtitle_rect = Rect::from_min_max(pos2(left, subtitle_top), pos2(right, top + block));
        painter.galley(
            crate::bidi::galley_pos(subtitle_rect, &subtitle),
            subtitle,
            colours.secondary,
        );
        if response.clicked() {
            app.actions.push(Action::PlayCommandPaletteEntry(index));
        }
    }
    // The pointer chooses as the arrow keys do, so Enter plays the row
    // under it. Only a moving pointer does: a list that changes under a
    // resting pointer keeps the keyboard's choice.
    if let Some(index) = hovered_row
        && let Some(palette) = &mut app.command_palette
    {
        palette.highlighted = index;
    }
}

fn centered(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), 24.0),
        Layout::centered_and_justified(egui::Direction::LeftToRight),
        add,
    );
}

/// The keys the palette answers to, and a spinner while Spotify answers.
fn footer(ui: &mut egui::Ui, colours: &theme::Palette, locale: Locale, busy: bool) {
    let rect = ui.available_rect_before_wrap();
    ui.painter().hline(
        rect.left()..=rect.right(),
        rect.top(),
        Stroke::new(1.0, colours.outline),
    );
    ui.add_space(PADDING);
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        theme::text(
            ui,
            // Translators: Keep the key names. Translate the words between them.
            gettext(locale, "↑ ↓ choose   ↵ play   Esc close"),
            theme::regular(12.0),
            colours.dim,
        );
        if busy {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(6.0);
                theme::spinner(ui, 14.0, colours.accent);
            });
        }
    });
}

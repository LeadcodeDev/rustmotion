use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::sidebar::{Sidebar, SidebarMenu, SidebarMenuItem};
use gpui_component::{h_flex, v_flex, ActiveTheme, IconName, Side, Sizable as _, StyledExt as _};
use gpui_kit::{
    div, px, App, AppContext as _, Context, Entity, InteractiveElement, IntoElement, ParentElement,
    Pixels, Render, RenderImage, StatefulInteractiveElement, Styled, Subscription, Window,
};

use crate::app::state::StudioState;
use crate::editor::frames::render_frame_rgba_deep;
use crate::editor::surface::{frame_from_rgba, frame_surface};
use crate::library::{ScenarioEntry, SharedLibrary};
use crate::scenario::{empty_scenario, StudioModel, View};

const SIDEBAR_WIDTH: Pixels = px(280.);
const CARD_MIN_WIDTH: Pixels = px(240.);
const CARD_GAP: Pixels = px(16.);
const CONTENT_PADDING_X: Pixels = px(24.);

const NEW_SCENARIO_TEMPLATE: &str = r##"{
  "video": { "width": 1920, "height": 1080, "background": "#0f172a" },
  "scenes": [
    { "duration": 3.0, "children": [
      { "type": "text", "content": "New scenario", "style": { "font-size": 64, "color": "#ffffff", "text-align": "center" } }
    ] }
  ]
}
"##;

struct Section {
    name: String,
    entries: Vec<ScenarioEntry>,
}

fn workspace_sections(library: &SharedLibrary) -> Vec<Section> {
    let lib = library.lock().unwrap_or_else(|e| e.into_inner());
    let mut sections = Vec::new();
    if !lib.recents.is_empty() {
        sections.push(Section {
            name: "Recent".to_string(),
            entries: lib.recents.clone(),
        });
    }
    for group in &lib.groups {
        sections.push(Section {
            name: group.name.clone(),
            entries: group.entries.clone(),
        });
    }
    sections
}

fn thumbnail_rgba(path: &Path) -> Option<Arc<RenderImage>> {
    let scenario = rustmotion::loader::load_input(&path.to_path_buf()).ok()?;
    let tasks = rustmotion::encode::build_frame_tasks(&scenario);
    if tasks.is_empty() {
        return None;
    }
    let (width, height, rgba) = render_frame_rgba_deep(&scenario, &tasks, 0, 0.25).ok()?;
    if width == 0 || height == 0 {
        return None;
    }
    Some(frame_from_rgba(width, height, rgba))
}

fn grid_columns(content_width: f32, card_width: f32, gap: f32) -> u16 {
    if content_width <= 0.0 || card_width <= 0.0 {
        return 1;
    }
    let columns = ((content_width + gap) / (card_width + gap)).floor();
    if columns < 1.0 {
        1
    } else {
        columns as u16
    }
}

fn open_scenario(state: &Entity<StudioState>, path: PathBuf, cx: &mut App) {
    let (shared, library) = {
        let studio = state.read(cx);
        (studio.shared.clone(), studio.library.clone())
    };
    let (scenario, error) = match rustmotion::loader::load_input(&path) {
        Ok(scenario) => (scenario, None),
        Err(e) => (empty_scenario(), Some(e.to_string())),
    };
    {
        let mut model = shared.lock().unwrap_or_else(|e| e.into_inner());
        let generation = model.generation.wrapping_add(1);
        *model = StudioModel::new(scenario, error, Some(path.clone()));
        model.generation = generation;
    }
    {
        let mut lib = library.lock().unwrap_or_else(|e| e.into_inner());
        lib.note_opened(&path);
        lib.retarget_watch(&path);
    }
    state.update(cx, |studio, cx| {
        studio.view = View::Editor;
        cx.notify();
    });
}

fn deduped_untitled_path(workspace: &Path) -> PathBuf {
    let mut path = workspace.join("untitled.json");
    let mut n = 1;
    while path.exists() {
        path = workspace.join(format!("untitled-{n}.json"));
        n += 1;
    }
    path
}

fn new_scenario(state: &Entity<StudioState>, cx: &mut App) {
    let workspace = {
        let studio = state.read(cx);
        let library = studio.library.lock().unwrap_or_else(|e| e.into_inner());
        library.workspace.clone()
    };
    let path = deduped_untitled_path(&workspace);
    if std::fs::write(&path, NEW_SCENARIO_TEMPLATE).is_ok() {
        open_scenario(state, path, cx);
    }
}

fn import_scenario(state: Entity<StudioState>, cx: &mut App) {
    cx.spawn(async move |cx| {
        let picked = rfd::AsyncFileDialog::new()
            .add_filter("scenario", &["json", "html", "htm"])
            .pick_file()
            .await;
        if let Some(handle) = picked {
            let path = handle.path().to_path_buf();
            cx.update(|cx| open_scenario(&state, path, cx));
        }
    })
    .detach();
}

fn card(
    entry: &ScenarioEntry,
    thumbnail: Option<Arc<RenderImage>>,
    state: &Entity<StudioState>,
    cx: &App,
) -> impl IntoElement {
    let card_id = format!("library-card-{}", entry.path.display());
    let click_state = state.clone();
    let click_path = entry.path.clone();

    v_flex()
        .id(card_id)
        .w_full()
        .gap_2()
        .cursor_pointer()
        .on_click(move |_, _, cx| open_scenario(&click_state, click_path.clone(), cx))
        .child(
            div()
                .w_full()
                .aspect_ratio(16.0 / 9.0)
                .overflow_hidden()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().muted)
                .child(frame_surface(thumbnail)),
        )
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().foreground)
                .truncate()
                .child(entry.name.clone()),
        )
}

pub struct Library {
    state: Entity<StudioState>,
    search: Entity<InputState>,
    selected: usize,
    query: String,
    thumbnails: HashMap<PathBuf, Arc<RenderImage>>,
    _subscriptions: Vec<Subscription>,
}

impl Library {
    pub fn new(state: Entity<StudioState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let query_subscription =
            cx.subscribe_in(&search, window, |this, input, event: &InputEvent, _, cx| {
                if let InputEvent::Change = event {
                    this.query = input.read(cx).value().to_string();
                    cx.notify();
                }
            });
        Self {
            state,
            search,
            selected: 0,
            query: String::new(),
            thumbnails: HashMap::new(),
            _subscriptions: vec![query_subscription],
        }
    }

    fn sync_thumbnails(
        &mut self,
        visible: &[ScenarioEntry],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keep: HashSet<&PathBuf> = visible.iter().map(|entry| &entry.path).collect();
        let stale: Vec<PathBuf> = self
            .thumbnails
            .keys()
            .filter(|path| !keep.contains(path))
            .cloned()
            .collect();
        for path in stale {
            if let Some(image) = self.thumbnails.remove(&path) {
                cx.drop_image(image, Some(window));
            }
        }
        for entry in visible {
            if !self.thumbnails.contains_key(&entry.path) {
                if let Some(image) = thumbnail_rgba(&entry.path) {
                    self.thumbnails.insert(entry.path.clone(), image);
                }
            }
        }
    }
}

impl Render for Library {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let library = self.state.read(cx).library.clone();
        let sections = workspace_sections(&library);

        if sections.is_empty() {
            self.selected = 0;
        } else if self.selected >= sections.len() {
            self.selected = sections.len() - 1;
        }

        let query = self.query.to_lowercase();
        let active_entries: Vec<ScenarioEntry> = sections
            .get(self.selected)
            .map(|section| {
                section
                    .entries
                    .iter()
                    .filter(|entry| query.is_empty() || entry.name.to_lowercase().contains(&query))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();

        self.sync_thumbnails(&active_entries, window, cx);

        let section_name = sections
            .get(self.selected)
            .map(|section| section.name.clone())
            .unwrap_or_default();

        let mut menu = SidebarMenu::new();
        for (index, section) in sections.iter().enumerate() {
            let count = section.entries.len();
            let is_active = index == self.selected;
            menu = menu.child(
                SidebarMenuItem::new(section.name.clone())
                    .icon(IconName::Folder)
                    .active(is_active)
                    .suffix(move |_, cx| {
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(count.to_string())
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected = index;
                        cx.notify();
                    })),
            );
        }

        let header = v_flex()
            .gap_3()
            .w_full()
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_lg()
                            .font_semibold()
                            .text_color(cx.theme().sidebar_foreground)
                            .child("Rustmotion"),
                    )
                    .child(
                        Button::new("library-new")
                            .icon(IconName::Plus)
                            .label("New")
                            .primary()
                            .small()
                            .on_click({
                                let state = self.state.clone();
                                move |_, _, cx| new_scenario(&state, cx)
                            }),
                    ),
            )
            .child(
                Button::new("library-import")
                    .icon(IconName::FolderOpen)
                    .label("Import file…")
                    .secondary()
                    .small()
                    .w_full()
                    .on_click({
                        let state = self.state.clone();
                        move |_, _, cx| import_scenario(state.clone(), cx)
                    }),
            );

        let sidebar = Sidebar::new("library-sidebar")
            .side(Side::Left)
            .w(SIDEBAR_WIDTH)
            .collapsible(false)
            .collapsed(false)
            .header(header)
            .child(menu);

        let content_width = (window.viewport_size().width.as_f32()
            - SIDEBAR_WIDTH.as_f32()
            - CONTENT_PADDING_X.as_f32() * 2.0)
            .max(0.0);
        let columns = grid_columns(content_width, CARD_MIN_WIDTH.as_f32(), CARD_GAP.as_f32());

        let state = self.state.clone();
        let grid_body = if active_entries.is_empty() {
            div()
                .text_color(cx.theme().muted_foreground)
                .child("No scenarios here.")
        } else {
            let cx_ref: &App = cx;
            let cards = active_entries.iter().map(|entry| {
                let thumbnail = self.thumbnails.get(&entry.path).cloned();
                card(entry, thumbnail, &state, cx_ref)
            });
            div()
                .grid()
                .grid_cols(columns)
                .gap(CARD_GAP)
                .children(cards)
        };

        h_flex()
            .id("library")
            .size_full()
            .bg(cx.theme().background)
            .child(sidebar)
            .child(
                v_flex()
                    .flex_1()
                    .h_full()
                    .overflow_hidden()
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .gap_4()
                            .px(CONTENT_PADDING_X)
                            .py_5()
                            .child(
                                div()
                                    .text_xl()
                                    .font_semibold()
                                    .text_color(cx.theme().foreground)
                                    .child(section_name),
                            )
                            .child(
                                Input::new(&self.search)
                                    .prefix(IconName::Search)
                                    .w(px(260.)),
                            ),
                    )
                    .child(
                        div()
                            .id("library-grid")
                            .flex_1()
                            .overflow_y_scroll()
                            .px(CONTENT_PADDING_X)
                            .pb(CONTENT_PADDING_X)
                            .child(grid_body),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use crate::library::LibraryState;

    fn examples_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
    }

    #[test]
    fn new_scenario_template_is_a_valid_scenario() {
        let result =
            rustmotion::loader::load_scenario_from_source(None, Some(NEW_SCENARIO_TEMPLATE));
        assert!(result.is_ok());
    }

    #[test]
    fn workspace_sections_puts_recent_first_when_present() {
        let mut state = LibraryState::new(examples_dir(), false);
        state.recents = vec![ScenarioEntry {
            path: PathBuf::from("/tmp/rm_library_view_recent_demo.json"),
            name: "recent-demo".to_string(),
            flat_index: 0,
        }];
        let library: SharedLibrary = Arc::new(Mutex::new(state));

        let sections = workspace_sections(&library);

        assert_eq!(sections.first().map(|s| s.name.as_str()), Some("Recent"));
        assert!(sections.iter().any(|s| s.name == "Default"));
    }

    #[test]
    fn workspace_sections_omits_recent_when_empty() {
        let mut state = LibraryState::new(examples_dir(), false);
        state.recents.clear();
        let library: SharedLibrary = Arc::new(Mutex::new(state));

        let sections = workspace_sections(&library);

        assert!(sections.iter().all(|s| s.name != "Recent"));
    }

    #[test]
    fn grid_columns_fits_as_many_240px_cards_as_the_width_allows() {
        assert_eq!(grid_columns(240.0, 240.0, 16.0), 1);
        assert_eq!(grid_columns(496.0, 240.0, 16.0), 2);
        assert_eq!(grid_columns(0.0, 240.0, 16.0), 1);
    }

    #[test]
    fn deduped_untitled_path_skips_existing_files() {
        let dir = std::env::temp_dir().join(format!("rm_library_view_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("untitled.json"), "{}").unwrap();
        std::fs::write(dir.join("untitled-1.json"), "{}").unwrap();

        let path = deduped_untitled_path(&dir);
        assert_eq!(path, dir.join("untitled-2.json"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn thumbnail_rgba_renders_a_quarter_scale_frame() {
        let dir = std::env::temp_dir().join(format!("rm_library_thumb_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("scenario.json");
        std::fs::write(
            &path,
            r##"{ "video": { "width": 400, "height": 200, "background": "#101418" }, "scenes": [ { "duration": 1.0 } ] }"##,
        )
        .unwrap();

        let image = thumbnail_rgba(&path).expect("a thumbnail for a valid scenario");
        let size = image.size(0);
        assert_eq!((size.width.0, size.height.0), (100, 50));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn thumbnail_rgba_rejects_a_scenario_with_no_frames() {
        let dir =
            std::env::temp_dir().join(format!("rm_library_thumb_empty_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("scenario.json");
        std::fs::write(
            &path,
            r##"{ "video": { "width": 400, "height": 200 }, "scenes": [] }"##,
        )
        .unwrap();

        assert!(thumbnail_rgba(&path).is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }
}

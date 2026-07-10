use crate::browser;
use crate::model::{
    Channel, EditorDocument, EditorMode, PositionPolicy, Projection, SourceKind, pose_skeleton,
};
use avatar_anim::SkeletonDefinition;
use leptos::ev::{DragEvent, Event, KeyboardEvent, PointerEvent};
use leptos::prelude::*;
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use wasm_bindgen_futures::spawn_local;
use web_sys::{File, HtmlInputElement};

type DocumentSignal = RwSignal<Option<EditorDocument>>;
type NoticeSignal = RwSignal<Option<(bool, String)>>;
type HistorySignal = RwSignal<HistoryState>;

const HISTORY_LIMIT: usize = 100;

#[derive(Clone, Default)]
struct HistoryState {
    past: Vec<EditorDocument>,
    present: Option<EditorDocument>,
    future: Vec<EditorDocument>,
    gesture_baseline: Option<EditorDocument>,
}

impl HistoryState {
    fn reset(&mut self, document: Option<EditorDocument>) {
        self.past.clear();
        self.present = document;
        self.future.clear();
        self.gesture_baseline = None;
    }

    fn observe(&mut self, document: Option<EditorDocument>) {
        if self.gesture_baseline.is_some() {
            return;
        }
        if history_content_eq(self.present.as_ref(), document.as_ref()) {
            self.present = document;
            return;
        }
        let previous = self.present.take();
        self.push_past(previous);
        self.present = document;
        self.future.clear();
    }

    fn begin_gesture(&mut self, document: Option<EditorDocument>) {
        self.observe(document.clone());
        self.gesture_baseline = document;
    }

    fn finish_gesture(&mut self, document: Option<EditorDocument>) {
        let Some(baseline) = self.gesture_baseline.take() else {
            self.observe(document);
            return;
        };
        if history_content_eq(Some(&baseline), document.as_ref()) {
            self.present = document;
            return;
        }
        self.push_past(Some(baseline));
        self.present = document;
        self.future.clear();
    }

    fn undo(&mut self, current: Option<EditorDocument>) -> Option<EditorDocument> {
        let mut target = self.past.pop()?;
        if let Some(current) = current {
            preserve_editor_state(&mut target, &current);
            self.future.push(current);
        }
        self.present = Some(target.clone());
        self.gesture_baseline = None;
        Some(target)
    }

    fn redo(&mut self, current: Option<EditorDocument>) -> Option<EditorDocument> {
        let mut target = self.future.pop()?;
        if let Some(current) = current {
            preserve_editor_state(&mut target, &current);
            self.push_past(Some(current));
        }
        self.present = Some(target.clone());
        self.gesture_baseline = None;
        Some(target)
    }

    fn push_past(&mut self, document: Option<EditorDocument>) {
        let Some(document) = document else {
            return;
        };
        self.past.push(document);
        if self.past.len() > HISTORY_LIMIT {
            self.past.remove(0);
        }
    }
}

fn history_content_eq(left: Option<&EditorDocument>, right: Option<&EditorDocument>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => {
            left.name == right.name
                && left.animation == right.animation
                && left.source == right.source
                && left.position_policy == right.position_policy
                && left.add_base_positions == right.add_base_positions
                && left.priority == right.priority
        }
        (None, None) => true,
        _ => false,
    }
}

fn preserve_editor_state(target: &mut EditorDocument, current: &EditorDocument) {
    target.mode = current.mode;
    target.selected_joint.clone_from(&current.selected_joint);
    target.current_time = current.current_time;
}

fn replace_document(document: DocumentSignal, history: HistorySignal, value: EditorDocument) {
    history.update(|state| state.reset(Some(value.clone())));
    document.set(Some(value));
}

fn undo(document: DocumentSignal, history: HistorySignal) {
    let current = document.get_untracked();
    if let Some(target) = history.try_update(|state| state.undo(current)).flatten() {
        document.set(Some(target));
    }
}

fn redo(document: DocumentSignal, history: HistorySignal) {
    let current = document.get_untracked();
    if let Some(target) = history.try_update(|state| state.redo(current)).flatten() {
        document.set(Some(target));
    }
}

#[component]
pub fn App() -> impl IntoView {
    let skeleton = Arc::new(
        SkeletonDefinition::embedded_avatar().expect("bundled avatar skeleton must be valid"),
    );
    let document = RwSignal::new(None::<EditorDocument>);
    let notice = RwSignal::new(None::<(bool, String)>);
    let dragging = RwSignal::new(false);
    let history = RwSignal::new(HistoryState::default());

    Effect::new(move |_| {
        let current = document.get();
        history.update(|state| state.observe(current));
    });

    let on_drag_over = move |event: DragEvent| {
        event.prevent_default();
        if let Some(transfer) = event.data_transfer() {
            transfer.set_drop_effect("copy");
        }
        dragging.set(true);
    };
    let on_drag_leave = move |_event: DragEvent| dragging.set(false);
    let skeleton_for_drop = Arc::clone(&skeleton);
    let on_drop = move |event: DragEvent| {
        event.prevent_default();
        dragging.set(false);
        if let Some(file) = event
            .data_transfer()
            .and_then(|transfer| transfer.files())
            .and_then(|files| files.get(0))
        {
            import_file(
                file,
                document,
                notice,
                Arc::clone(&skeleton_for_drop),
                history,
            );
        }
    };
    let skeleton_for_input = Arc::clone(&skeleton);
    let on_file_input = move |event: Event| {
        let input: HtmlInputElement = event_target(&event);
        if let Some(file) = input.files().and_then(|files| files.get(0)) {
            import_file(
                file,
                document,
                notice,
                Arc::clone(&skeleton_for_input),
                history,
            );
        }
        input.set_value("");
    };
    let on_key_down = move |event: KeyboardEvent| {
        if !(event.ctrl_key() || event.meta_key()) || event.alt_key() {
            return;
        }
        match event.key().to_ascii_lowercase().as_str() {
            "z" if event.shift_key() => {
                event.prevent_default();
                redo(document, history);
            }
            "z" => {
                event.prevent_default();
                undo(document, history);
            }
            "y" => {
                event.prevent_default();
                redo(document, history);
            }
            _ => {}
        }
    };

    view! {
        <div
            class:dragging=move || dragging.get()
            class="app-shell"
            on:dragover=on_drag_over
            on:dragleave=on_drag_leave
            on:drop=on_drop
            on:keydown=on_key_down
            on:pointerdown=move |_| history.update(|state| state.begin_gesture(document.get_untracked()))
            on:pointerup=move |_| history.update(|state| state.finish_gesture(document.get_untracked()))
            on:pointercancel=move |_| history.update(|state| state.finish_gesture(document.get_untracked()))
        >
            <input
                id="global-file-input"
                class="visually-hidden"
                type="file"
                accept=".xml,.bvh,.anim,application/xml,application/octet-stream"
                on:change=on_file_input
            />
            <Header document skeleton=Arc::clone(&skeleton) notice />

            <Show
                when=move || document.get().is_some()
                fallback=move || view! {
                    <Welcome document notice dragging history />
                }
            >
                <Editor document skeleton=Arc::clone(&skeleton) notice history />
            </Show>

            <Show when=move || notice.get().is_some()>
                {move || notice.get().map(|(is_error, message)| view! {
                    <div
                        class="toast"
                        class:error=is_error
                        role=if is_error { "alert" } else { "status" }
                    >
                        <span>{message}</span>
                        <button
                            class="icon-button"
                            aria-label="Dismiss message"
                            on:click=move |_| notice.set(None)
                        >"×"</button>
                    </div>
                })}
            </Show>

            <div class="drop-overlay" aria-hidden="true">
                <div class="drop-overlay-card">
                    <span class="drop-icon">"↓"</span>
                    <strong>"Drop to open"</strong>
                    <span>"Firestorm XML/BVH or Second Life ANIM"</span>
                </div>
            </div>
        </div>
    }
}

#[component]
fn Header(
    document: DocumentSignal,
    skeleton: Arc<SkeletonDefinition>,
    notice: NoticeSignal,
) -> impl IntoView {
    let download = move |_| {
        let Some(doc) = document.get() else {
            return;
        };
        match doc.export(&skeleton) {
            Ok(bytes) => match browser::download(&doc.output_name(), &bytes) {
                Ok(()) => notice.set(Some((false, format!("Downloaded {}", doc.output_name())))),
                Err(error) => notice.set(Some((true, error))),
            },
            Err(error) => notice.set(Some((true, format!("Could not export: {error}")))),
        }
    };

    view! {
        <header class="topbar">
            <div class="brand">
                <div class="brand-mark" aria-hidden="true">
                    <span></span><span></span><span></span>
                </div>
                <div>
                    <strong>"Avatar Anim Studio"</strong>
                    <small>"Second Life pose & animation editor"</small>
                </div>
            </div>
            <div class="topbar-actions">
                <label class="button secondary" for="global-file-input">"Open file"</label>
                <button
                    class="button primary"
                    disabled=move || document.get().is_none()
                    on:click=download
                >
                    "Download .anim"
                </button>
            </div>
        </header>
    }
}

#[component]
fn Welcome(
    document: DocumentSignal,
    notice: NoticeSignal,
    dragging: RwSignal<bool>,
    history: HistorySignal,
) -> impl IntoView {
    view! {
        <main class="welcome">
            <section class="welcome-copy">
                <div class="eyebrow">"PRIVATE · LOCAL · NO INSTALL"</div>
                <h1>"Turn a Firestorm pose into a clean animation."</h1>
                <p>
                    "Open a poser XML, BVH animation, or existing .anim. Inspect the exact bones, tune rotations in degrees, preview the skeleton, and download a viewer-ready file. Everything stays in this browser."
                </p>
                <div class="welcome-actions">
                    <label class="button primary large" for="global-file-input">"Choose pose or animation"</label>
                    <button class="button ghost large" on:click=move |_| {
                        replace_document(document, history, EditorDocument::blank());
                        notice.set(Some((false, "Started a blank pose".to_owned())));
                    }>
                        "Start a blank pose"
                    </button>
                </div>
            </section>

            <label
                class="drop-zone"
                class:active=move || dragging.get()
                for="global-file-input"
            >
                <div class="drop-illustration" aria-hidden="true">
                    <div class="source-file-cards">
                        <div class="file-card xml">"XML"</div>
                        <div class="file-card bvh">"BVH"</div>
                    </div>
                    <div class="transfer-arrow">"→"</div>
                    <div class="file-card anim">"ANIM"</div>
                </div>
                <strong>"Drop your file here"</strong>
                <span>"or click to browse · .xml, .bvh, and .anim supported"</span>
            </label>

            <details class="file-help">
                <summary>
                    <span class="help-icon" aria-hidden="true">"?"</span>
                    <span><strong>"Where does Firestorm save pose XML/BVH files?"</strong><small>"Show locations for Linux, Windows, and macOS"</small></span>
                    <span class="summary-chevron" aria-hidden="true">"⌄"</span>
                </summary>
                <div class="path-grid">
                    <div class="path-card">
                        <span class="os-name">"Linux"</span>
                        <code>"~/.firestorm_x64/user_settings/poses/"</code>
                    </div>
                    <div class="path-card">
                        <span class="os-name">"Windows"</span>
                        <code>"%APPDATA%\\Firestorm_x64\\user_settings\\poses\\"</code>
                    </div>
                    <div class="path-card">
                        <span class="os-name">"macOS"</span>
                        <code>"~/Library/Application Support/Firestorm_x64/user_settings/poses/"</code>
                    </div>
                </div>
                <p>"Save a pose from Firestorm's Poser, then choose its .xml or optional .bvh file here. Hand presets may be inside a poses/hand_presets subfolder."</p>
            </details>

            <section class="preflight" aria-labelledby="preflight-title">
                <div>
                    <div class="eyebrow">"BEFORE EXPORTING FROM FIRESTORM"</div>
                    <h2 id="preflight-title">"A reliable pose starts clean."</h2>
                </div>
                <ol>
                    <li><span>"1"</span><div><strong>"Apply Zero Pose"</strong><p>"Clear inherited rotations before shaping your pose."</p></div></li>
                    <li><span>"2"</span><div><strong>"Move only what you need"</strong><p>"Enabled bones are imported; unchanged channels can be removed here."</p></div></li>
                    <li><span>"3"</span><div><strong>"Save XML or BVH"</strong><p>"Drop that file above. The LL skeleton is already included."</p></div></li>
                </ol>
            </section>
        </main>
    }
}

#[component]
fn Editor(
    document: DocumentSignal,
    skeleton: Arc<SkeletonDefinition>,
    notice: NoticeSignal,
    history: HistorySignal,
) -> impl IntoView {
    let search = RwSignal::new(String::new());
    let active_only = RwSignal::new(true);
    let projection = RwSignal::new(Projection::Front);
    let hidden_groups = RwSignal::new(BTreeSet::<String>::new());

    view! {
        <main class="editor-page">
            <section class="document-bar">
                <div class="document-meta">
                    <span class="status-dot"></span>
                    <div>
                        <strong>{move || document.get().map(|doc| doc.name).unwrap_or_default()}</strong>
                        <small>{move || document.get().map(|doc| {
                            let source = match doc.source {
                                SourceKind::Blank => "Blank pose",
                                SourceKind::Firestorm => "Firestorm poser XML",
                                SourceKind::Bvh => "BVH animation",
                                SourceKind::Animation => "Second Life animation",
                            };
                            format!("{source} · {} active joints · {} keys", doc.animation.joints.len(), doc.key_count())
                        }).unwrap_or_default()}</small>
                    </div>
                </div>
                <div class="document-tools">
                    <HistoryControls document history />
                    <ModeSwitch document />
                </div>
                <ExportSettings document skeleton=Arc::clone(&skeleton) notice />
            </section>

            <div class="workspace">
                <JointBrowser
                    document
                    skeleton=Arc::clone(&skeleton)
                    search
                    active_only
                    hidden_groups
                />
                <section class="stage-column">
                    <SkeletonStage
                        document
                        skeleton=Arc::clone(&skeleton)
                        projection
                        hidden_groups
                    />
                    <Show when=move || document.get().is_some_and(|doc| doc.mode == EditorMode::Animation)>
                        <Timeline document />
                    </Show>
                </section>
                <Inspector document skeleton=Arc::clone(&skeleton) />
            </div>
        </main>
    }
}

#[component]
fn HistoryControls(document: DocumentSignal, history: HistorySignal) -> impl IntoView {
    view! {
        <div class="history-controls" role="group" aria-label="Edit history">
            <button
                type="button"
                title="Undo (Ctrl+Z)"
                aria-label="Undo"
                disabled=move || history.get().past.is_empty()
                on:click=move |_| undo(document, history)
            >
                <span aria-hidden="true">"↶"</span>
            </button>
            <button
                type="button"
                title="Redo (Ctrl+Shift+Z or Ctrl+Y)"
                aria-label="Redo"
                disabled=move || history.get().future.is_empty()
                on:click=move |_| redo(document, history)
            >
                <span aria-hidden="true">"↷"</span>
            </button>
        </div>
    }
}

#[component]
fn ModeSwitch(document: DocumentSignal) -> impl IntoView {
    view! {
        <div class="mode-switch" role="tablist" aria-label="Editor mode">
            <button
                role="tab"
                class:active=move || document.get().is_some_and(|doc| doc.mode == EditorMode::Pose)
                aria-selected=move || document.get().is_some_and(|doc| doc.mode == EditorMode::Pose)
                on:click=move |_| document.update(|state| if let Some(doc) = state {
                    doc.mode = EditorMode::Pose;
                    doc.current_time = u16::MAX;
                })
            >
                <span class="mode-icon">"◆"</span>
                <span><strong>"Pose"</strong><small>"Single frame"</small></span>
            </button>
            <button
                role="tab"
                class:active=move || document.get().is_some_and(|doc| doc.mode == EditorMode::Animation)
                aria-selected=move || document.get().is_some_and(|doc| doc.mode == EditorMode::Animation)
                on:click=move |_| document.update(|state| if let Some(doc) = state {
                    doc.mode = EditorMode::Animation;
                    doc.current_time = 0;
                })
            >
                <span class="mode-icon keyframes">"◆ ◆"</span>
                <span><strong>"Keyframes"</strong><small>"Timeline"</small></span>
            </button>
        </div>
    }
}

#[component]
fn ExportSettings(
    document: DocumentSignal,
    skeleton: Arc<SkeletonDefinition>,
    notice: NoticeSignal,
) -> impl IntoView {
    let download = move |_| {
        let Some(doc) = document.get() else {
            return;
        };
        match doc.export(&skeleton) {
            Ok(bytes) => match browser::download(&doc.output_name(), &bytes) {
                Ok(()) => notice.set(Some((false, format!("Downloaded {}", doc.output_name())))),
                Err(error) => notice.set(Some((true, error))),
            },
            Err(error) => notice.set(Some((true, format!("Could not export: {error}")))),
        }
    };

    view! {
        <div class="quick-export">
            <label>
                <span>"Priority"</span>
                <select
                    prop:value=move || document.get().map(|doc| doc.priority.to_string()).unwrap_or_else(|| "4".to_owned())
                    on:change=move |event| {
                        let value = event_target_value(&event).parse::<i32>().unwrap_or(4);
                        document.update(|state| if let Some(doc) = state { doc.priority = value; });
                    }
                >
                    {(0..=7).map(|priority| view! { <option value=priority>{priority}</option> }).collect_view()}
                </select>
            </label>
            <label class="position-policy">
                <span>"Positions"</span>
                <select
                    title="Choose which position deltas are written"
                    prop:value=move || document.get().map_or("none", |doc| match doc.position_policy {
                        PositionPolicy::None => "none",
                        PositionPolicy::NonZero => "non-zero",
                        PositionPolicy::All => "all",
                    })
                    on:change=move |event| {
                        let policy = match event_target_value(&event).as_str() {
                            "non-zero" => PositionPolicy::NonZero,
                            "all" => PositionPolicy::All,
                            _ => PositionPolicy::None,
                        };
                        document.update(|state| if let Some(doc) = state { doc.position_policy = policy; });
                    }
                >
                    <option value="none">"None"</option>
                    <option value="non-zero">"Non-zero deltas"</option>
                    <option value="all">"All deltas"</option>
                </select>
            </label>
            <label
                class="compact-check"
                title="Convert deltas to the local LL skeleton positions expected by .anim files"
            >
                <input
                    type="checkbox"
                    disabled=move || document.get().is_none_or(|doc| doc.position_policy == PositionPolicy::None)
                    prop:checked=move || document.get().is_some_and(|doc| doc.add_base_positions)
                    on:change=move |event| {
                        let checked = event_target_checked(&event);
                        document.update(|state| if let Some(doc) = state { doc.add_base_positions = checked; });
                    }
                />
                <span>"Add LL base"</span>
            </label>
            <button class="button primary compact" on:click=download>"Export"</button>
        </div>
    }
}

#[component]
fn JointBrowser(
    document: DocumentSignal,
    skeleton: Arc<SkeletonDefinition>,
    search: RwSignal<String>,
    active_only: RwSignal<bool>,
    hidden_groups: RwSignal<BTreeSet<String>>,
) -> impl IntoView {
    let skeleton_for_list = Arc::clone(&skeleton);
    let visible_names = move || {
        let Some(doc) = document.get() else {
            return Vec::new();
        };
        let query = search.get().to_ascii_lowercase();
        let hidden = hidden_groups.get();
        doc.joint_names(&skeleton_for_list)
            .into_iter()
            .filter(|name| !joint_is_hidden(&skeleton_for_list, name, &hidden))
            .filter(|name| !active_only.get() || doc.joint_is_active(name))
            .filter(|name| query.is_empty() || name.to_ascii_lowercase().contains(&query))
            .collect::<Vec<_>>()
    };

    view! {
        <aside class="panel joint-browser">
            <div class="panel-heading">
                <div><span class="eyebrow">"RIG"</span><h2>"Joints"</h2></div>
                <span class="count-badge">{move || document.get().map(|doc| doc.animation.joints.len()).unwrap_or(0)}</span>
            </div>
            <label class="search-box">
                <span aria-hidden="true">"⌕"</span>
                <input
                    type="search"
                    placeholder="Find a joint"
                    prop:value=move || search.get()
                    on:input=move |event| search.set(event_target_value(&event))
                />
            </label>
            <div class="filter-row">
                <button
                    class:active=move || active_only.get()
                    on:click=move |_| active_only.set(true)
                >"Active"</button>
                <button
                    class:active=move || !active_only.get()
                    on:click=move |_| active_only.set(false)
                >"All bones"</button>
            </div>
            <div class="joint-list">
                <For
                    each=visible_names
                    key=|name| name.clone()
                    children=move |name| {
                        let select_name = name.clone();
                        let selected_name = name.clone();
                        let active_name = name.clone();
                        let rotation_name = name.clone();
                        let position_name = name.clone();
                        view! {
                            <button
                                class="joint-row"
                                class:selected=move || document.get().is_some_and(|doc| doc.selected_joint.as_deref() == Some(selected_name.as_str()))
                                class:inactive=move || !document.get().is_some_and(|doc| doc.joint_is_active(&active_name))
                                on:click=move |_| document.update(|state| if let Some(doc) = state {
                                    doc.selected_joint = Some(select_name.clone());
                                })
                            >
                                <span class="joint-led"></span>
                                <span class="joint-name">{name}</span>
                                <span class="channel-pips">
                                    <span class:enabled=move || document.get().is_some_and(|doc| doc.channel_enabled(&rotation_name, Channel::Rotation))>"R"</span>
                                    <span class:enabled=move || document.get().is_some_and(|doc| doc.channel_enabled(&position_name, Channel::Position))>"P"</span>
                                </span>
                            </button>
                        }
                    }
                />
            </div>
        </aside>
    }
}

#[derive(Clone, Copy)]
struct RigGroup {
    label: &'static str,
    groups: &'static [&'static str],
}

const RIG_GROUPS: [RigGroup; 8] = [
    RigGroup {
        label: "Face",
        groups: &["Face", "Lips", "Mouth", "Nose", "Eyes", "Ears"],
    },
    RigGroup {
        label: "Tail",
        groups: &["Tail"],
    },
    RigGroup {
        label: "Wings",
        groups: &["Wing"],
    },
    RigGroup {
        label: "Hind limbs",
        groups: &["Limb"],
    },
    RigGroup {
        label: "Groin",
        groups: &["Groin"],
    },
    RigGroup {
        label: "Hands",
        groups: &["Hand"],
    },
    RigGroup {
        label: "Other extras",
        groups: &["Extra"],
    },
    RigGroup {
        label: "Collision volumes",
        groups: &["Collision"],
    },
];

#[component]
fn RigVisibility(hidden_groups: RwSignal<BTreeSet<String>>) -> impl IntoView {
    view! {
        <details class="rig-visibility">
            <summary title="Show or hide optional rig groups">
                <span aria-hidden="true">"◉"</span>
                <span>"Rig display"</span>
                <Show when=move || !hidden_groups.get().is_empty()>
                    <i>{move || {
                        RIG_GROUPS
                            .iter()
                            .filter(|preset| preset.groups.iter().any(|group| hidden_groups.get().contains(*group)))
                            .count()
                    }}</i>
                </Show>
            </summary>
            <div class="rig-visibility-menu">
                <div class="visibility-heading">
                    <div><strong>"Rig visibility"</strong><small>"Display only · animation data is retained"</small></div>
                    <button
                        type="button"
                        on:click=move |_| hidden_groups.set(BTreeSet::new())
                    >"Show all"</button>
                </div>
                {RIG_GROUPS.into_iter().map(|preset| {
                    let groups = preset.groups;
                    view! {
                        <label>
                            <input
                                type="checkbox"
                                prop:checked=move || {
                                    let hidden = hidden_groups.get();
                                    groups.iter().all(|group| !hidden.contains(*group))
                                }
                                on:change=move |event| {
                                    let visible = event_target_checked(&event);
                                    hidden_groups.update(|hidden| for group in groups {
                                        if visible {
                                            hidden.remove(*group);
                                        } else {
                                            hidden.insert((*group).to_owned());
                                        }
                                    });
                                }
                            />
                            <span>{preset.label}</span>
                        </label>
                    }
                }).collect_view()}
            </div>
        </details>
    }
}

fn joint_is_hidden(
    skeleton: &SkeletonDefinition,
    name: &str,
    hidden_groups: &BTreeSet<String>,
) -> bool {
    skeleton
        .bone(name)
        .and_then(|bone| bone.attributes.get("group"))
        .is_some_and(|group| hidden_groups.contains(group))
}

#[component]
fn SkeletonStage(
    document: DocumentSignal,
    skeleton: Arc<SkeletonDefinition>,
    projection: RwSignal<Projection>,
    hidden_groups: RwSignal<BTreeSet<String>>,
) -> impl IntoView {
    let orbit_yaw = RwSignal::new(0.55_f32);
    let orbit_pitch = RwSignal::new(-0.12_f32);
    let orbit_drag = RwSignal::new(None::<OrbitDrag>);
    let gizmo_drag = RwSignal::new(None::<GizmoDrag>);

    view! {
        <section class="stage-card">
            <div class="stage-toolbar">
                <div>
                    <span class="eyebrow">"LIVE SKELETON"</span>
                    <h2>"Pose preview"</h2>
                </div>
                <div class="stage-controls">
                    <RigVisibility hidden_groups />
                    <div class="segmented" role="group" aria-label="Preview angle">
                        <button
                            class:active=move || projection.get() == Projection::Front
                            on:click=move |_| projection.set(Projection::Front)
                        >"Front"</button>
                        <button
                            class:active=move || projection.get() == Projection::Side
                            on:click=move |_| projection.set(Projection::Side)
                        >"Side"</button>
                        <button
                            class:active=move || projection.get() == Projection::Orbit
                            on:click=move |_| projection.set(Projection::Orbit)
                        >"Orbit"</button>
                    </div>
                </div>
            </div>
            <div class="skeleton-canvas">
                {move || {
                    let Some(doc) = document.get() else {
                        return ().into_any();
                    };
                    let hidden = hidden_groups.get();
                    let poses = pose_skeleton(&doc, &skeleton)
                        .into_iter()
                        .filter(|pose| !joint_is_hidden(&skeleton, &pose.name, &hidden))
                        .collect::<Vec<_>>();
                    let projected = project_bones(
                        &poses,
                        projection.get(),
                        orbit_yaw.get(),
                        orbit_pitch.get(),
                    );
                    let points: HashMap<String, (f32, f32)> = projected
                        .iter()
                        .map(|bone| (bone.name.clone(), (bone.x, bone.y)))
                        .collect();
                    let selected_point = doc
                        .selected_joint
                        .as_ref()
                        .and_then(|name| points.get(name))
                        .copied();
                    let lines = projected.iter().filter_map(|bone| {
                        let parent = bone.parent.as_ref().and_then(|name| points.get(name))?;
                        Some(view! {
                            <line
                                x1=parent.0
                                y1=parent.1
                                x2=bone.x
                                y2=bone.y
                                class:active=bone.active
                            />
                        })
                    }).collect_view();
                    let nodes = projected.into_iter().map(|bone| {
                        let click_name = bone.name.clone();
                        let keyboard_name = bone.name.clone();
                        let label_name = bone.name.clone();
                        let title_name = bone.name.clone();
                        let selected = doc.selected_joint.as_deref() == Some(bone.name.as_str());
                        view! {
                            <g
                                class="bone-node"
                                class:active=bone.active
                                class:selected=selected
                                role="button"
                                tabindex="0"
                                aria-label=format!("Select {label_name}")
                                on:click=move |_| document.update(|state| if let Some(doc) = state {
                                    doc.selected_joint = Some(click_name.clone());
                                })
                                on:keydown=move |event: KeyboardEvent| {
                                    if event.key() == "Enter" || event.key() == " " {
                                        event.prevent_default();
                                        let name = keyboard_name.clone();
                                        document.update(move |state| if let Some(doc) = state {
                                            doc.selected_joint = Some(name);
                                        });
                                    }
                                }
                            >
                                <circle cx=bone.x cy=bone.y r=if selected { 6.5 } else if bone.active { 4.5 } else { 2.4 } />
                                <title>{title_name}</title>
                            </g>
                        }
                    }).collect_view();
                    let gizmo = selected_point.and_then(|(x, y)| {
                        let name = doc.selected_joint.clone()?;
                        let rotation = doc.rotation_degrees(&name);
                        Some(rotation_gizmo(x, y, name, rotation, gizmo_drag))
                    });
                    view! {
                        <svg
                            class:orbit=move || projection.get() == Projection::Orbit
                            viewBox="0 0 480 620"
                            role="img"
                            aria-label="Interactive avatar skeleton preview"
                            on:pointermove=move |event: PointerEvent| {
                                if let Some(drag) = gizmo_drag.get() {
                                    let degrees = drag.start_degrees
                                        + (event.client_x() as f32 - drag.start_x) * 0.55;
                                    document.update(|state| if let Some(doc) = state {
                                        doc.set_rotation_component(&drag.joint, drag.axis, degrees);
                                    });
                                } else if let Some(drag) = orbit_drag.get() {
                                    let dx = event.client_x() as f32 - drag.start_x;
                                    let dy = event.client_y() as f32 - drag.start_y;
                                    orbit_yaw.set(drag.start_yaw + dx * 0.008);
                                    orbit_pitch.set((drag.start_pitch + dy * 0.008).clamp(-1.35, 1.35));
                                }
                            }
                            on:pointerup=move |_| {
                                orbit_drag.set(None);
                                gizmo_drag.set(None);
                            }
                            on:pointerleave=move |_| {
                                orbit_drag.set(None);
                                gizmo_drag.set(None);
                            }
                        >
                            <rect
                                class="orbit-hit-area"
                                x="0"
                                y="0"
                                width="480"
                                height="620"
                                on:pointerdown=move |event: PointerEvent| {
                                    if projection.get() == Projection::Orbit {
                                        event.prevent_default();
                                        orbit_drag.set(Some(OrbitDrag {
                                            start_x: event.client_x() as f32,
                                            start_y: event.client_y() as f32,
                                            start_yaw: orbit_yaw.get_untracked(),
                                            start_pitch: orbit_pitch.get_untracked(),
                                        }));
                                    }
                                }
                            />
                            <g class="skeleton-grid">
                                <line x1="240" y1="22" x2="240" y2="598" />
                                <line x1="24" y1="570" x2="456" y2="570" />
                            </g>
                            <g class="bone-lines">{lines}</g>
                            <g class="bone-nodes">{nodes}</g>
                            {gizmo}
                        </svg>
                    }.into_any()
                }}
                <div class="canvas-legend">
                    <Show when=move || projection.get() == Projection::Orbit>
                        <span class="orbit-help">"Drag background to orbit · drag a gizmo ring to rotate"</span>
                    </Show>
                    <span><i class="active"></i>"Animated"</span>
                    <span><i></i>"Reference"</span>
                </div>
            </div>
        </section>
    }
}

#[derive(Clone, Copy)]
struct OrbitDrag {
    start_x: f32,
    start_y: f32,
    start_yaw: f32,
    start_pitch: f32,
}

#[derive(Clone)]
struct GizmoDrag {
    axis: usize,
    start_x: f32,
    start_degrees: f32,
    joint: String,
}

fn rotation_gizmo(
    x: f32,
    y: f32,
    joint: String,
    rotation: glam::Vec3,
    gizmo_drag: RwSignal<Option<GizmoDrag>>,
) -> impl IntoView {
    let axes = [
        (0_usize, "x", "X", 0_f32, x + 45.0, y - 2.0),
        (1_usize, "y", "Y", 60_f32, x - 27.0, y - 34.0),
        (2_usize, "z", "Z", 120_f32, x - 27.0, y + 39.0),
    ];

    view! {
        <g class="rotation-gizmo" aria-label=format!("Rotation gizmo for {joint}")>
            <circle class="gizmo-center" cx=x cy=y r="7" />
            {axes.into_iter().map(|(axis, class, label, angle, label_x, label_y)| {
                let drag_joint = joint.clone();
                let start_degrees = rotation[axis];
                view! {
                    <g
                        class=format!("gizmo-axis gizmo-{class}")
                        role="slider"
                        aria-label=format!("Rotate {label} axis")
                        aria-valuenow=format!("{start_degrees:.2}")
                        on:pointerdown=move |event: PointerEvent| {
                            event.prevent_default();
                            gizmo_drag.set(Some(GizmoDrag {
                                axis,
                                start_x: event.client_x() as f32,
                                start_degrees,
                                joint: drag_joint.clone(),
                            }));
                        }
                    >
                        <ellipse
                            class="gizmo-hit"
                            cx=x
                            cy=y
                            rx="42"
                            ry="14"
                            transform=format!("rotate({angle} {x} {y})")
                        />
                        <ellipse
                            class="gizmo-ring"
                            cx=x
                            cy=y
                            rx="42"
                            ry="14"
                            transform=format!("rotate({angle} {x} {y})")
                        />
                        <text x=label_x y=label_y>{label}</text>
                    </g>
                }
            }).collect_view()}
        </g>
    }
}

struct ProjectedBone {
    name: String,
    parent: Option<String>,
    x: f32,
    y: f32,
    active: bool,
}

fn project_bones(
    poses: &[crate::model::BonePose],
    projection: Projection,
    orbit_yaw: f32,
    orbit_pitch: f32,
) -> Vec<ProjectedBone> {
    let orbit_rotation =
        glam::Quat::from_rotation_x(orbit_pitch) * glam::Quat::from_rotation_z(orbit_yaw);
    let raw = poses
        .iter()
        .map(|bone| match projection {
            Projection::Front => (-bone.world_position.y, bone.world_position.z),
            Projection::Side => (bone.world_position.x, bone.world_position.z),
            Projection::Orbit => {
                let position = orbit_rotation * bone.world_position;
                (-position.y, position.z)
            }
        })
        .collect::<Vec<_>>();
    let (min_x, max_x, min_y, max_y) = raw.iter().fold(
        (f32::MAX, f32::MIN, f32::MAX, f32::MIN),
        |(min_x, max_x, min_y, max_y), (x, y)| {
            (min_x.min(*x), max_x.max(*x), min_y.min(*y), max_y.max(*y))
        },
    );
    let width = (max_x - min_x).max(0.01);
    let height = (max_y - min_y).max(0.01);
    let scale = (410.0 / width).min(550.0 / height);
    let offset_x = 240.0 - (min_x + max_x) * 0.5 * scale;
    let offset_y = 310.0 + (min_y + max_y) * 0.5 * scale;

    poses
        .iter()
        .zip(raw)
        .map(|(bone, (x, y))| ProjectedBone {
            name: bone.name.clone(),
            parent: bone.parent.clone(),
            x: x * scale + offset_x,
            y: offset_y - y * scale,
            active: bone.active,
        })
        .collect()
}

#[component]
fn Inspector(document: DocumentSignal, skeleton: Arc<SkeletonDefinition>) -> impl IntoView {
    view! {
        <aside class="panel inspector">
            {move || {
                let Some(doc) = document.get() else {
                    return ().into_any();
                };
                let Some(name) = doc.selected_joint.clone() else {
                    return view! {
                        <div class="empty-inspector"><span>"◇"</span><h2>"Select a joint"</h2><p>"Choose a bone in the list or preview."</p></div>
                    }.into_any();
                };
                let bone = skeleton.bone(&name);
                let parent = bone.and_then(|bone| bone.parent.as_deref()).unwrap_or("Root");
                let group = bone
                    .and_then(|bone| bone.attributes.get("group"))
                    .map(String::as_str)
                    .unwrap_or("Imported");
                let rotation_enabled = doc.channel_enabled(&name, Channel::Rotation);
                let position_enabled = doc.channel_enabled(&name, Channel::Position);
                let is_active = rotation_enabled || position_enabled;
                let rotation = doc.rotation_degrees(&name);
                let position = doc.position(&name);

                let rotation_name = name.clone();
                let position_name = name.clone();
                let remove_button = if is_active {
                    let remove_name = name.clone();
                    view! {
                        <button
                            class="icon-button danger"
                            title="Remove this joint from the animation"
                            aria-label="Remove this joint"
                            on:click=move |_| {
                                let name = remove_name.clone();
                                document.update(move |state| if let Some(doc) = state {
                                    doc.remove_joint(&name);
                                });
                            }
                        >"×"</button>
                    }.into_any()
                } else {
                    ().into_any()
                };
                let inactive_callout = if is_active {
                    ().into_any()
                } else {
                    let add_name = name.clone();
                    view! {
                        <div class="inactive-callout">
                            <strong>"Reference bone"</strong>
                            <p>"This joint is not written to the animation yet."</p>
                            <button class="button secondary full" on:click=move |_| {
                                let name = add_name.clone();
                                document.update(move |state| if let Some(doc) = state {
                                    doc.toggle_channel(&name, Channel::Rotation, true);
                                });
                            }>"Add rotation channel"</button>
                        </div>
                    }.into_any()
                };
                view! {
                    <div class="inspector-heading">
                        <div><span class="eyebrow">{group.to_ascii_uppercase()}</span><h2>{name.clone()}</h2><small>{format!("Child of {parent}")}</small></div>
                        {remove_button}
                    </div>

                    <ChannelEditor
                        title="Rotation"
                        description="Euler angles in degrees · XYZ order"
                        channel=Channel::Rotation
                        name=rotation_name
                        values=rotation
                        document
                        enabled=rotation_enabled
                    />
                    <ChannelEditor
                        title="Position delta"
                        description="Offset from the LL skeleton · metres"
                        channel=Channel::Position
                        name=position_name
                        values=position
                        document
                        enabled=position_enabled
                    />

                    {inactive_callout}
                }.into_any()
            }}
        </aside>
    }
}

#[component]
fn ChannelEditor(
    title: &'static str,
    description: &'static str,
    channel: Channel,
    name: String,
    values: glam::Vec3,
    document: DocumentSignal,
    enabled: bool,
) -> impl IntoView {
    let toggle_name = name.clone();
    view! {
        <section class="channel-card" class:disabled=!enabled>
            <div class="channel-heading">
                <div><h3>{title}</h3><p>{description}</p></div>
                <label class="switch">
                    <input
                        type="checkbox"
                        aria-label=format!("Enable {title}")
                        prop:checked=enabled
                        on:change=move |event| {
                            let checked = event_target_checked(&event);
                            document.update(|state| if let Some(doc) = state {
                                doc.toggle_channel(&toggle_name, channel, checked);
                            });
                        }
                    />
                    <span></span>
                </label>
            </div>
            <div class="vector-grid">
                {[("X", values.x), ("Y", values.y), ("Z", values.z)]
                    .into_iter()
                    .enumerate()
                    .map(|(index, (axis, value))| {
                        let edit_name = name.clone();
                        view! {
                            <label class=format!("axis axis-{}", axis.to_ascii_lowercase())>
                                <span>{axis}</span>
                                <input
                                    type="number"
                                    step=if channel == Channel::Rotation { "1" } else { "0.001" }
                                    disabled=!enabled
                                    prop:value=format_value(value, channel)
                                    on:change=move |event| {
                                        if let Ok(value) = event_target_value(&event).parse::<f32>() {
                                            document.update(|state| if let Some(doc) = state {
                                                match channel {
                                                    Channel::Rotation => doc.set_rotation_component(&edit_name, index, value),
                                                    Channel::Position => doc.set_position_component(&edit_name, index, value),
                                                }
                                            });
                                        }
                                    }
                                />
                                <small>{if channel == Channel::Rotation { "°" } else { "m" }}</small>
                            </label>
                        }
                    })
                    .collect_view()}
            </div>
        </section>
    }
}

fn format_value(value: f32, channel: Channel) -> String {
    match channel {
        Channel::Rotation => format!("{value:.2}"),
        Channel::Position => format!("{value:.4}"),
    }
}

#[component]
fn Timeline(document: DocumentSignal) -> impl IntoView {
    view! {
        <section class="timeline-card">
            <div class="timeline-toolbar">
                <div>
                    <span class="eyebrow">"TIMELINE"</span>
                    <strong>{move || document.get().map(|doc| format!("{:.3} s", doc.time_seconds())).unwrap_or_default()}</strong>
                </div>
                <label class="duration-input">
                    <span>"Duration"</span>
                    <input
                        type="number"
                        min="0.017"
                        step="0.1"
                        prop:value=move || document.get().map(|doc| format!("{:.3}", doc.animation.header.duration)).unwrap_or_default()
                        on:change=move |event| {
                            if let Ok(value) = event_target_value(&event).parse::<f32>() {
                                document.update(|state| if let Some(doc) = state { doc.animation.set_duration(value); });
                            }
                        }
                    />
                    <small>"s"</small>
                </label>
                <button class="button ghost compact" on:click=move |_| document.update(|state| if let Some(doc) = state {
                    let time = doc.current_time;
                    for joint in &mut doc.animation.joints {
                        joint.rotation_keys.retain(|key| key.time != time);
                        joint.position_keys.retain(|key| key.time != time);
                    }
                    doc.animation.drop_empty_joints();
                })>"Delete keys at playhead"</button>
            </div>
            <div class="scrubber">
                <input
                    aria-label="Animation playhead"
                    type="range"
                    min="0"
                    max=u16::MAX
                    step="1"
                    prop:value=move || document.get().map(|doc| doc.current_time).unwrap_or(0)
                    on:input=move |event| {
                        let value = event_target_value(&event).parse::<u16>().unwrap_or(0);
                        document.update(|state| if let Some(doc) = state { doc.current_time = value; });
                    }
                />
                <div class="time-labels"><span>"0"</span><span>{move || document.get().map(|doc| format!("{:.2}s", doc.animation.header.duration)).unwrap_or_default()}</span></div>
            </div>
            <div class="track-list">
                {move || document.get().map(|doc| {
                    doc.animation.joints.iter().map(|joint| {
                        let name = joint.name.clone();
                        let select_name = name.clone();
                        let rotation_keys = joint.rotation_keys.iter().map(|key| (key.time, Channel::Rotation)).collect::<Vec<_>>();
                        let position_keys = joint.position_keys.iter().map(|key| (key.time, Channel::Position)).collect::<Vec<_>>();
                        view! {
                            <div class="track-row">
                                <button class="track-name" on:click=move |_| document.update(|state| if let Some(doc) = state {
                                    doc.selected_joint = Some(select_name.clone());
                                })>{name}</button>
                                <div class="track-lane">
                                    {rotation_keys.into_iter().chain(position_keys).map(|(time, channel)| {
                                        let key_name = joint.name.clone();
                                        view! {
                                            <button
                                                class="key-dot"
                                                class:position=channel == Channel::Position
                                                class:selected=doc.current_time == time
                                                style:left=format!("{:.4}%", time as f32 / u16::MAX as f32 * 100.0)
                                                title=format!("{} key at {:.3}s", if channel == Channel::Rotation { "Rotation" } else { "Position" }, time as f32 / u16::MAX as f32 * doc.animation.header.duration)
                                                on:click=move |_| document.update(|state| if let Some(doc) = state {
                                                    doc.current_time = time;
                                                    doc.selected_joint = Some(key_name.clone());
                                                })
                                            ></button>
                                        }
                                    }).collect_view()}
                                </div>
                            </div>
                        }
                    }).collect_view()
                })}
            </div>
            <p class="timeline-hint">"Move the playhead, then edit a value to create or replace a key at that time."</p>
        </section>
    }
}

fn import_file(
    file: File,
    document: DocumentSignal,
    notice: NoticeSignal,
    skeleton: Arc<SkeletonDefinition>,
    history: HistorySignal,
) {
    spawn_local(async move {
        match browser::read_file(file).await {
            Ok((name, bytes)) => match EditorDocument::from_file(name.clone(), &bytes, &skeleton) {
                Ok(doc) => {
                    let summary = match doc.source {
                        SourceKind::Firestorm => format!("Opened {name} in Pose mode"),
                        SourceKind::Bvh => format!("Opened {name} as BVH"),
                        SourceKind::Animation => format!("Opened {name} in Keyframes mode"),
                        SourceKind::Blank => format!("Opened {name}"),
                    };
                    replace_document(document, history, doc);
                    notice.set(Some((false, summary)));
                }
                Err(error) => notice.set(Some((true, error))),
            },
            Err(error) => notice.set(Some((true, format!("Could not read file: {error}")))),
        }
    });
}

#[cfg(test)]
mod history_tests {
    use super::*;

    #[test]
    fn undo_and_redo_restore_content_but_keep_editor_navigation() {
        let original = EditorDocument::blank();
        let mut history = HistoryState::default();
        history.reset(Some(original.clone()));

        let mut edited = original;
        edited.set_rotation_component("mPelvis", 0, 30.0);
        edited.selected_joint = Some("mHead".to_owned());
        history.observe(Some(edited.clone()));

        let undone = history.undo(Some(edited.clone())).unwrap();
        assert!(undone.rotation_degrees("mPelvis").x.abs() < 0.01);
        assert_eq!(undone.selected_joint.as_deref(), Some("mHead"));

        let redone = history.redo(Some(undone)).unwrap();
        assert!((redone.rotation_degrees("mPelvis").x - 30.0).abs() < 0.01);
    }

    #[test]
    fn continuous_gesture_creates_one_history_step() {
        let original = EditorDocument::blank();
        let mut history = HistoryState::default();
        history.reset(Some(original.clone()));
        history.begin_gesture(Some(original));

        let mut edited = history.gesture_baseline.clone().unwrap();
        for degrees in [1.0, 10.0, 25.0] {
            edited.set_rotation_component("mPelvis", 1, degrees);
            history.observe(Some(edited.clone()));
        }
        history.finish_gesture(Some(edited.clone()));

        assert_eq!(history.past.len(), 1);
        let undone = history.undo(Some(edited)).unwrap();
        assert!(undone.rotation_degrees("mPelvis").y.abs() < 0.01);
    }

    #[test]
    fn selection_and_playhead_changes_do_not_consume_history() {
        let original = EditorDocument::blank();
        let mut history = HistoryState::default();
        history.reset(Some(original.clone()));

        let mut navigated = original;
        navigated.selected_joint = Some("mHead".to_owned());
        navigated.current_time = 12_345;
        history.observe(Some(navigated));

        assert!(history.past.is_empty());
        assert!(history.future.is_empty());
    }
}

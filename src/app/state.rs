use crate::storage::SearchExecution;
use crate::*;
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) struct SearchRequest {
    pub(crate) query_generation: u64,
    pub(crate) query: String,
    pub(crate) pasta_brain: bool,
    pub(crate) execution: SearchExecution,
}

pub(crate) struct SearchResponse {
    pub(crate) query_generation: u64,
    pub(crate) execution: SearchExecution,
    pub(crate) items: Vec<ClipboardRecord>,
    pub(crate) row_presentations: Vec<CachedRowPresentation>,
}

pub(crate) struct TextInputState {
    pub(crate) focus_handle: FocusHandle,
    pub(crate) selected_range: Range<usize>,
    pub(crate) selection_reversed: bool,
    pub(crate) marked_range: Option<Range<usize>>,
    pub(crate) last_layout: Option<ShapedLine>,
    pub(crate) last_bounds: Option<Bounds<Pixels>>,
    pub(crate) is_selecting: bool,
}

impl TextInputState {
    pub(crate) fn new<T>(cx: &mut Context<T>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            is_selecting: false,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.selected_range = 0..0;
        self.selection_reversed = false;
        self.marked_range = None;
        self.last_layout = None;
        self.last_bounds = None;
        self.is_selecting = false;
    }

    pub(crate) fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }
}

pub(crate) struct CachedRowPresentation {
    pub(crate) title: String,
    pub(crate) created_label: String,
    pub(crate) detected_language: Option<LanguageTag>,
    pub(crate) collapsed_preview: String,
    pub(crate) expanded_preview: String,
    pub(crate) expanded_preview_line_count: usize,
    pub(crate) expanded_preview_truncated: bool,
    pub(crate) masked_preview: String,
}

impl CachedRowPresentation {
    pub(crate) fn from_record(item: &ClipboardRecord) -> Self {
        if let Some(image) = &item.image {
            // Image attachments have no meaningful text body — skip the
            // language/preview/title machinery built for text content.
            let metadata = format_image_metadata(image);
            return Self {
                title: metadata.clone(),
                created_label: format_timestamp(&item.created_at),
                detected_language: None,
                collapsed_preview: metadata.clone(),
                expanded_preview: metadata,
                expanded_preview_line_count: 1,
                expanded_preview_truncated: false,
                masked_preview: String::new(),
            };
        }

        let detected_language = detect_language(item.item_type, &item.content);

        let expanded_preview_full = expanded_preview_content(&item.content);
        let (expanded_preview, expanded_preview_truncated) =
            bounded_preview_content(&expanded_preview_full, PREVIEW_PANE_TEXT_LIMIT);
        let expanded_preview_line_count = expanded_preview.lines().count();
        let collapsed_preview = preview_content(&item.content);
        let title = single_line_title(&item.content, &collapsed_preview);

        Self {
            title,
            created_label: format_timestamp(&item.created_at),
            detected_language,
            collapsed_preview,
            expanded_preview,
            expanded_preview_line_count,
            expanded_preview_truncated,
            masked_preview: masked_secret_preview(&item.content),
        }
    }

    pub(crate) fn collect(items: &[ClipboardRecord]) -> Vec<Self> {
        items.iter().map(Self::from_record).collect()
    }
}

/// Collapse a clip's content to a single trimmed line suitable for a dense list
/// row: the first non-blank line, with interior runs of whitespace squeezed so
/// leading indentation or tabs never blow out the row width.
fn single_line_title(content: &str, fallback: &str) -> String {
    let source = content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_else(|| fallback.lines().next().unwrap_or("").trim());
    let collapsed = source.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        "Empty".to_owned()
    } else {
        collapsed
    }
}

pub(crate) fn start_search_worker(
    storage: Arc<ClipboardStorage>,
) -> (
    mpsc::Sender<SearchRequest>,
    futures::channel::mpsc::UnboundedReceiver<SearchResponse>,
    Arc<AtomicU64>,
) {
    let (request_tx, request_rx) = mpsc::channel::<SearchRequest>();
    let (result_tx, result_rx) = futures::channel::mpsc::unbounded::<SearchResponse>();
    let latest_query_generation = Arc::new(AtomicU64::new(0));

    let cancel_generation = latest_query_generation.clone();
    let spawn_result = std::thread::Builder::new()
        .name("pasta-search-worker".to_owned())
        .spawn(move || {
            while let Ok(mut request) = request_rx.recv() {
                while let Ok(newer) = request_rx.try_recv() {
                    request = newer;
                }

                let items = storage
                    .search_items(
                        &request.query,
                        48,
                        request.pasta_brain,
                        request.execution,
                        request.query_generation,
                        Some(cancel_generation.as_ref()),
                    )
                    .unwrap_or_else(|err| {
                        eprintln!("warning: search worker failed to query clipboard items: {err}");
                        Vec::new()
                    });
                if cancel_generation.load(Ordering::Acquire) != request.query_generation {
                    continue;
                }
                let row_presentations = CachedRowPresentation::collect(&items);

                if result_tx
                    .unbounded_send(SearchResponse {
                        query_generation: request.query_generation,
                        execution: request.execution,
                        items,
                        row_presentations,
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
    if let Err(err) = spawn_result {
        eprintln!("warning: failed to start search worker thread: {err}");
    }

    (request_tx, result_rx, latest_query_generation)
}

pub(crate) struct LauncherView {
    pub(crate) storage: Arc<ClipboardStorage>,
    pub(crate) ui_font_family: SharedString,
    pub(crate) content_font_family: SharedString,
    pub(crate) surface_alpha: f32,
    pub(crate) pasta_brain_enabled: bool,
    pub(crate) query_input_state: TextInputState,
    pub(crate) info_editor_input_state: TextInputState,
    pub(crate) tag_editor_input_state: TextInputState,
    pub(crate) bowl_editor_input_state: TextInputState,
    pub(crate) parameter_name_input_state: TextInputState,
    pub(crate) parameter_fill_input_state: TextInputState,
    pub(crate) pending_text_input_focus: Option<TextInputTarget>,
    pub(crate) results_scroll: UniformListScrollHandle,
    pub(crate) search_request_tx: mpsc::Sender<SearchRequest>,
    pub(crate) search_generation: u64,
    pub(crate) search_generation_token: Arc<AtomicU64>,
    pub(crate) latest_applied_search_execution: SearchExecution,
    pub(crate) query: String,
    pub(crate) last_query_edit_at: Option<Instant>,
    pub(crate) tag_search_suggestions: Vec<String>,
    pub(crate) items: Vec<ClipboardRecord>,
    pub(crate) row_presentations: Vec<CachedRowPresentation>,
    pub(crate) selected_index: usize,
    pub(crate) selection_changed_at: Instant,
    pub(crate) transition_alpha: f32,
    pub(crate) transition_from: f32,
    pub(crate) transition_target: f32,
    pub(crate) transition_started_at: Instant,
    pub(crate) transition_duration: Duration,
    pub(crate) pending_exit: Option<LauncherExitIntent>,
    pub(crate) revealed_secret_id: Option<i64>,
    pub(crate) reveal_until: Option<Instant>,
    pub(crate) last_reveal_second_bucket: Option<u64>,
    pub(crate) info_editor_target_id: Option<i64>,
    pub(crate) info_editor_input: String,
    pub(crate) info_editor_select_all: bool,
    pub(crate) tag_editor_target_id: Option<i64>,
    pub(crate) tag_editor_input: String,
    pub(crate) tag_editor_select_all: bool,
    pub(crate) tag_editor_mode: TagEditorMode,
    pub(crate) bowl_editor_target_id: Option<i64>,
    pub(crate) bowl_editor_input: String,
    pub(crate) bowl_editor_select_all: bool,
    pub(crate) bowl_editor_suggestions: Vec<String>,
    pub(crate) parameter_editor_target_id: Option<i64>,
    pub(crate) parameter_editor_stage: ParameterEditorStage,
    pub(crate) parameter_editor_force_full: bool,
    pub(crate) parameter_editor_selected_targets: Vec<String>,
    pub(crate) parameter_editor_name_inputs: Vec<String>,
    pub(crate) parameter_editor_name_focus_index: usize,
    pub(crate) parameter_editor_name_select_all: bool,
    pub(crate) parameter_editor_split_tokens: HashSet<String>,
    pub(crate) parameter_fill_target_id: Option<i64>,
    pub(crate) parameter_fill_values: Vec<String>,
    pub(crate) parameter_fill_focus_index: usize,
    pub(crate) parameter_fill_select_all: bool,
    pub(crate) transform_menu_open: bool,
    pub(crate) qr_preview: Option<(i64, QrMatrix)>,
    pub(crate) blur_close_armed: bool,
    pub(crate) pending_blur_hide_at: Option<Instant>,
    pub(crate) suppress_auto_hide: bool,
    pub(crate) suppress_auto_hide_until: Option<Instant>,
    pub(crate) pinned: bool,
    pub(crate) show_command_help: bool,
    pub(crate) caret_visible: bool,
    pub(crate) caret_blink_due_at: Instant,
    pub(crate) emoji_search_active: bool,
    pub(crate) emoji_search_results: Vec<usize>,
    pub(crate) emoji_search_selected_index: usize,
    pub(crate) emoji_results_scroll: UniformListScrollHandle,
}

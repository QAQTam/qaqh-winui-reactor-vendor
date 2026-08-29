use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};

use rustc_hash::{FxHashMap, FxHashSet};

use super::*;

mod convert;
mod diag;
mod generated_attach_event;
mod generated_set_prop;
use convert::*;

/// Keeps `Handle`, `ControlKind` construction, and diagnostics in one table.
macro_rules! define_handles {
    ( $( $variant:ident ),* $(,)? ) => {
        enum Handle {
            $( $variant(bindings::$variant), )*
        }

        impl Handle {
            fn cast_inner<T: windows_core::Interface>(&self) -> windows_core::Result<T> {
                match self {
                    $( Handle::$variant(v) => v.cast::<T>(), )*
                }
            }
            fn as_framework_element(&self) -> bindings::FrameworkElement {
                self.cast_inner().unwrap()
            }
            fn as_ui_element(&self) -> bindings::UIElement {
                self.cast_inner().unwrap()
            }
            fn kind_name(&self) -> &'static str {
                match self {
                    $( Handle::$variant(_) => stringify!($variant), )*
                }
            }
        }

        impl WinUIBackend {
            fn make_handle_for_kind(kind: ControlKind) -> Handle {
                match kind {
                    $(
                        ControlKind::$variant => Handle::$variant(
                            <bindings::$variant>::new().unwrap(),
                        ),
                    )*
                }
            }
        }

        fn describe_kind(h: &Handle) -> &'static str {
            match h {
                $( Handle::$variant(_) => stringify!($variant), )*
            }
        }
    };
}

define_handles! {
    AutoSuggestBox,
    Border,
    BreadcrumbBar,
    Button,
    CalendarDatePicker,
    CalendarView,
    Canvas,
    CheckBox,
    ColorPicker,
    ComboBox,
    CommandBar,
    ContentDialog,
    DatePicker,
    DropDownButton,
    Ellipse,
    Expander,
    FlipView,
    Grid,
    GridView,
    HyperlinkButton,
    Image,
    InfoBadge,
    InfoBar,
    Line,
    ListBox,
    ListView,
    MenuBar,
    NavigationView,
    NumberBox,
    PasswordBox,
    PersonPicture,
    Pivot,
    PivotItem,
    ProgressBar,
    ProgressRing,
    RadioButton,
    RadioButtons,
    RatingControl,
    Rectangle,
    RelativePanel,
    RepeatButton,
    RichEditBox,
    RichTextBlock,
    ScrollView,
    ScrollViewer,
    SelectorBar,
    Slider,
    SplitButton,
    SplitView,
    StackPanel,
    SwapChainPanel,
    TabView,
    TabViewItem,
    TeachingTip,
    TextBlock,
    TextBox,
    TimePicker,
    TitleBar,
    ToggleButton,
    ToggleSwitch,
    TreeView,
    Viewbox,
    WebView2,
}

/// [`Backend`] implementation that creates real `Microsoft.UI.Xaml`
/// controls and drives them on the WinUI thread.
pub struct WinUIBackend {
    controls: RefCell<FxHashMap<ControlId, Handle>>,
    event_revokers: RefCell<FxHashMap<(ControlId, Event), Vec<EventRevoker>>>,
    property_observers: RefCell<FxHashMap<(ControlId, Event), PropertyObserver>>,
    templated_selection_revokers: RefCell<FxHashMap<ControlId, EventRevoker>>,
    /// Per-list virtualization state for templated ListView/GridView/FlipView.
    templated: RefCell<FxHashMap<ControlId, TemplatedList>>,
    /// Shared ListView/GridView template; its root `ContentControl` can host
    /// reactor elements where `ListViewItemPresenter` would render strings.
    content_template: RefCell<Option<bindings::DataTemplate>>,
    /// Last committed rich-text paragraphs per RichTextBlock control.
    /// `set_rich_text_paragraphs` diffs against this snapshot (paragraph
    /// level + per-paragraph run level) and only touches what changed —
    /// the reconciler re-runs element builds every frame during streaming,
    /// and unchanged blocks/runs would otherwise re-layout the whole text
    /// on the UI thread each time.
    rich_text: RefCell<FxHashMap<ControlId, RichTextBlockState>>,
    pointer_revokers: RefCell<FxHashMap<ControlId, PointerRevokerSet>>,
    drag_revokers: RefCell<FxHashMap<ControlId, DragRevokerSet>>,
    menu_click_handlers: RefCell<FxHashMap<ControlId, EventHandler>>,
    command_bar_flyout_handlers: RefCell<FxHashMap<ControlId, EventHandler>>,
    theme_brush_registry: RefCell<FxHashMap<ControlId, Vec<(Prop, ThemeRef)>>>,
    resource_keys: RefCell<FxHashMap<ControlId, FxHashSet<String>>>,
    /// Flyout open requests that arrived before the flyout itself was created
    /// (props are applied before the flyout content slot mounts). Consumed by
    /// `set_flyout_content` once the `Flyout` exists.
    flyout_open_pending: RefCell<FxHashMap<ControlId, bool>>,
    /// Flyout Closed handlers that arrived before the flyout existed.
    flyout_closed_pending: RefCell<FxHashMap<ControlId, EventHandler>>,
    /// ContentDialog 生命周期状态：关闭动画前清空内容/缩小尺寸（规避 WinUI
    /// 关闭残影），重新打开前据此恢复。`closing_attached` 防重复挂事件。
    /// Rc 包装：Closed 事件闭包（`attach_event`）需捕获它来复位 closing/shown。
    /// QAQ B2：Elevation z 值暂存（prop 阶段父未挂，receiver 在 insert_child 解析）。
    elevation: RefCell<FxHashMap<ControlId, f64>>,
    /// QAQ B2：已插入树并应用过投影的元素（生命周期内 set_prop 直接改 Translation）。
    elevation_live: RefCell<FxHashSet<ControlId>>,
    dialog_state: Rc<RefCell<FxHashMap<ControlId, DialogState>>>,
    /// 程序化关闭（✕）排队 Hide 的取消标志：IsOpen(true) 置 false 取消，
    /// 防止「排队 Hide 期间重新打开」把新对话框关掉。
    dialog_hide_pending: RefCell<FxHashMap<ControlId, Rc<AtomicBool>>>,
    /// Per-host window state for window-level props.
    window_state: RefCell<Option<Rc<HostWindowState>>>,
    next_id: RefCell<u32>,
}

/// ContentDialog 显示/关闭的生命周期快照。
#[derive(Default)]
struct DialogState {
    /// 挂载的内容子树（`set_content_element` 记录；重新打开前恢复）。
    content_id: Option<ControlId>,
    /// 渲染层最近一次设置的尺寸（`set_prop` 记录；关闭动画前会缩到 1x1）。
    width: Option<f64>,
    height: Option<f64>,
    /// Closing 清理事件是否已挂载（首次打开时挂一次）。
    closing_attached: bool,
    /// ShowAsync 已发起、对话框处于打开状态。IsOpen=false 仅在 shown 时
    /// 进入 closing，避免 Esc 后 on_closed 补发的 IsOpen=false（对话框已关）
    /// 误置 closing 导致下次打开被拦截。
    shown: bool,
    /// 关闭动画进行中（IsOpen=false 已应用、Closed 尚未触发）。此窗口内
    /// 收到 IsOpen=true 时跳过 ShowAsync：WinUI 拒绝动画中的 ShowAsync，
    /// 对话框会卡在「遮罩常驻、Closed 不触发」的僵死态且无法自愈。
    /// Closed 事件复位。
    closing: bool,
}

#[derive(Default)]
struct PointerRevokerSet {
    tapped: Option<EventRevoker>,
    right_tapped: Option<EventRevoker>,
    pressed: Option<EventRevoker>,
    released: Option<EventRevoker>,
    moved: Option<EventRevoker>,
    entered: Option<EventRevoker>,
    exited: Option<EventRevoker>,
    capture_lost: Option<EventRevoker>,
    canceled: Option<EventRevoker>,
    capture_on_press: bool,
}

struct PropertyObserver {
    object: bindings::DependencyObject,
    property: bindings::DependencyProperty,
    token: i64,
}

impl Drop for PropertyObserver {
    fn drop(&mut self) {
        diag::dropped(
            self.object
                .UnregisterPropertyChangedCallback(&self.property, self.token),
        );
    }
}

#[derive(Default)]
struct DragRevokerSet {
    enter: Option<EventRevoker>,
    leave: Option<EventRevoker>,
    over: Option<EventRevoker>,
    drop: Option<EventRevoker>,
}

/// Shared templated-list state touched from WinUI event handlers.
#[derive(Clone, Default)]
struct TemplatedShared {
    source: Rc<RefCell<Option<windows_collections::IObservableVector<windows_core::IInspectable>>>>,
    /// Logical row index -> template-root content host.
    containers: Rc<RefCell<FxHashMap<usize, bindings::IContentControl>>>,
    /// Logical row index -> native item container used for anchor geometry.
    item_containers: Rc<RefCell<FxHashMap<usize, bindings::IUIElement>>>,
    scroll: Rc<RefCell<TemplatedScrollState>>,
}

struct TemplatedScrollState {
    viewer: Option<bindings::IScrollViewer>,
    /// User intent, not merely the current geometric distance from the tail.
    /// Content growth may temporarily increase that distance before XAML has
    /// completed layout; only an upward viewport movement detaches the user.
    following_tail: bool,
    last_vertical_offset: f64,
    top_threshold: f64,
    tail_threshold: f64,
    on_top_reached: Option<Callback<()>>,
    on_view_changed: Option<Callback<TemplatedViewport>>,
    near_top: bool,
    pending: Option<PreparedTemplatedScroll>,
}

#[derive(Copy, Clone, Debug, PartialEq)]
enum PreparedTemplatedScroll {
    Tail,
    PreserveAnchor {
        index: usize,
        viewport_offset: f64,
        offset_before: f64,
    },
    RestoreOffset {
        vertical_offset: f64,
        following_tail: bool,
    },
}

impl Default for TemplatedScrollState {
    fn default() -> Self {
        Self {
            viewer: None,
            following_tail: true,
            last_vertical_offset: 0.0,
            top_threshold: 48.0,
            tail_threshold: 48.0,
            on_top_reached: None,
            on_view_changed: None,
            near_top: false,
            pending: None,
        }
    }
}

/// RichTextBlock 增量状态：内容快照 + 已 realize 的段落/run 对象。
///
/// `set_rich_text_paragraphs` 逐段比较：未变化段落完全不动（Blocks
/// 集合与 Paragraph 对象保留，不触发整块重排）；变化段复用段落对象，
/// 段内再逐 run 比较——相同前缀 run 对象原位保留，文本变化但样式相同
/// 的 run 复用对象仅 SetText（O(1)），样式变化的 run 新建并 SetAt 替换，
/// 新增 run Append，多余 run RemoveAtEnd。流式期间每帧只动最后一个
/// 生长 run，成本 O(delta) 而非 O(总长)。
#[derive(Default)]
struct RichTextBlockState {
    paragraphs: Vec<RichTextParagraph>,
    blocks: Vec<bindings::Paragraph>,
    runs: Vec<Vec<bindings::Run>>,
}

/// 除文本外的 run 样式是否相同（复用对象仅 SetText 的前提）。
fn rich_run_style_eq(a: &RichTextRun, b: &RichTextRun) -> bool {
    a.is_bold == b.is_bold
        && a.is_italic == b.is_italic
        && a.foreground == b.foreground
        && a.font_size == b.font_size
        && a.font_family == b.font_family
}

/// 新建一个 run 对象并设置文本 + 完整样式。
fn build_run_inline(def: &RichTextInline) -> Option<bindings::Run> {
    match def {
        RichTextInline::Run(r) => {
            let run = bindings::Run::new().ok()?;
            run.SetText(&r.text).ok()?;
            if r.is_bold {
                run.cast::<bindings::ITextElement>()
                    .and_then(|te| te.SetFontWeight(bindings::FontWeight { weight: 700 }))
                    .ok()?;
            }
            if r.is_italic {
                run.cast::<bindings::ITextElement>()
                    .and_then(|te| te.SetFontStyle(bindings::FontStyle::Italic))
                    .ok()?;
            }
            if let Some(foreground) = r.foreground {
                run.cast::<bindings::ITextElement>()
                    .and_then(|te| {
                        let brush = solid_brush(foreground)?;
                        te.SetForeground(&brush)
                    })
                    .ok()?;
            }
            if let Some(size) = r.font_size {
                run.cast::<bindings::ITextElement>()
                    .and_then(|te| te.SetFontSize(size))
                    .ok()?;
            }
            if let Some(family) = &r.font_family {
                run.cast::<bindings::ITextElement>()
                    .and_then(|te| {
                        let family = bindings::FontFamily::CreateInstanceWithName(family)?;
                        te.SetFontFamily(&family)
                    })
                    .ok()?;
            }
            Some(run)
        }
        RichTextInline::LineBreak => {
            let run = bindings::Run::new().ok()?;
            run.SetText("\n").ok()?;
            Some(run)
        }
        RichTextInline::Hyperlink(h) => {
            let run = bindings::Run::new().ok()?;
            run.SetText(&h.text).ok()?;
            Some(run)
        }
    }
}

/// 复用 run 对象：仅 SetText（调用方保证样式相同）。
fn apply_run_inline(run: &bindings::Run, def: &RichTextInline) {
    match def {
        RichTextInline::Run(r) => diag::dropped(run.SetText(&r.text)),
        RichTextInline::LineBreak => diag::dropped(run.SetText("\n")),
        RichTextInline::Hyperlink(h) => diag::dropped(run.SetText(&h.text)),
    }
}

/// 段内 run 级 diff：把 `new_defs` 同步进 `inlines`（对齐 `old_defs`）。
/// 相同前缀 run 对象原位保留；文本变化但样式相同的 run 复用对象仅
/// SetText；样式变化的 run 新建并 SetAt 替换；新增 Append；多余移除。
/// `runs` 记录本段已 realize 的 run 对象（与 Inlines 位置一一对应）。
/// 返回 (新建 run 数, 复用 run 数) 供诊断。
fn sync_paragraph_inlines(
    inlines: &bindings::InlineCollection,
    old_defs: &[RichTextInline],
    new_defs: &[RichTextInline],
    runs: &mut Vec<bindings::Run>,
) -> (usize, usize) {
    let mut new_runs = 0usize;
    let mut reused_runs = 0usize;
    let common = old_defs
        .iter()
        .zip(new_defs.iter())
        .take_while(|(a, b)| a == b)
        .count();
    for j in common..new_defs.len() {
        let def = &new_defs[j];
        if j < runs.len() {
            let style_same = match (&old_defs[j], def) {
                (RichTextInline::Run(a), RichTextInline::Run(b)) => rich_run_style_eq(a, b),
                (RichTextInline::LineBreak, RichTextInline::LineBreak) => true,
                (RichTextInline::Hyperlink(a), RichTextInline::Hyperlink(b)) => a.uri == b.uri,
                _ => false,
            };
            if style_same {
                apply_run_inline(&runs[j], def);
                reused_runs += 1;
            } else if let Some(run) = build_run_inline(def) {
                diag::dropped(
                    run.cast::<bindings::Inline>()
                        .and_then(|i| inlines.SetAt(j as u32, &i)),
                );
                runs[j] = run;
                new_runs += 1;
            }
        } else if let Some(run) = build_run_inline(def) {
            diag::dropped(
                run.cast::<bindings::Inline>()
                    .and_then(|i| inlines.Append(&i)),
            );
            runs.push(run);
            new_runs += 1;
        }
    }
    while runs.len() > new_defs.len() {
        diag::dropped(inlines.RemoveAtEnd());
        runs.pop();
    }
    (new_runs, reused_runs)
}

/// Per-list backend bookkeeping for templated (virtualized) lists.
struct TemplatedList {
    shared: TemplatedShared,
    realize_revoker: Option<EventRevoker>,
    reorder_revoker: Option<EventRevoker>,
    view_changed_revoker: Option<EventRevoker>,
    layout_updated_revoker: Option<EventRevoker>,
}

impl TemplatedList {
    fn new() -> Self {
        Self {
            shared: TemplatedShared::default(),
            realize_revoker: None,
            reorder_revoker: None,
            view_changed_revoker: None,
            layout_updated_revoker: None,
        }
    }
}

const TAIL_POSITION_EPSILON: f64 = 0.5;

/// Updates follow-tail intent from a completed/native view change.
///
/// A growing extent with an unchanged offset is a layout race, not evidence
/// that the user scrolled away. An actual upward offset movement detaches and
/// cancels a pending tail correction. Returning inside the threshold opts in
/// again and acknowledges a pending tail request.
fn observe_templated_view(state: &mut TemplatedScrollState, vertical: f64, scrollable: f64) {
    let distance = (scrollable - vertical).max(0.0);
    let moved_up = vertical + TAIL_POSITION_EPSILON < state.last_vertical_offset;

    if moved_up && distance > state.tail_threshold {
        state.following_tail = false;
        if matches!(state.pending, Some(PreparedTemplatedScroll::Tail)) {
            state.pending = None;
        }
    } else if distance <= state.tail_threshold {
        state.following_tail = true;
        if matches!(state.pending, Some(PreparedTemplatedScroll::Tail)) {
            state.pending = None;
        }
    }

    state.last_vertical_offset = vertical;
}

/// Applies a prepared request using geometry from the latest XAML layout.
///
/// Tail requests remain armed until `ViewChanged` confirms the final offset.
/// This is essential for streaming rows: reconcile runs before measure/arrange,
/// so an immediate `ChangeView` may target the previous `ScrollableHeight`.
fn apply_prepared_templated_scroll_shared(
    scroll: &Rc<RefCell<TemplatedScrollState>>,
    item_containers: &Rc<RefCell<FxHashMap<usize, bindings::IUIElement>>>,
) -> bool {
    let (request, viewer) = {
        let state = scroll.borrow();
        let Some(request) = state.pending else {
            return true;
        };
        let Some(viewer) = state.viewer.clone() else {
            return false;
        };
        (request, viewer)
    };

    let (target, wait_for_confirmation) = match request {
        PreparedTemplatedScroll::Tail => {
            let Ok(target) = viewer.ScrollableHeight() else {
                return false;
            };
            (target.max(0.0), true)
        }
        PreparedTemplatedScroll::PreserveAnchor {
            index,
            viewport_offset,
            offset_before,
        } => {
            let Some(container) = item_containers.borrow().get(&index).cloned() else {
                return false;
            };
            let Ok(offset) = container.ActualOffset() else {
                return false;
            };
            (
                (offset_before + f64::from(offset.y) - viewport_offset).max(0.0),
                false,
            )
        }
        PreparedTemplatedScroll::RestoreOffset {
            vertical_offset, ..
        } => (vertical_offset.max(0.0), false),
    };

    let current = viewer.VerticalOffset().unwrap_or(0.0);
    let scrollable = viewer.ScrollableHeight().unwrap_or(0.0);
    // Content not yet laid out (ScrollableHeight == 0) must not confirm a
    // Tail request: target 0 == current 0 would silently "succeed", the
    // request is dropped, and the list stays pinned at the top once the
    // extent grows — restore of a large snapshot on the first frame hits
    // this every time (reconcile runs before measure/arrange). Keep the
    // request armed; the LayoutUpdated observer retries with the final
    // ScrollableHeight of a later layout pass.
    let tail_requires_layout =
        matches!(request, PreparedTemplatedScroll::Tail) && scrollable <= TAIL_POSITION_EPSILON;
    if !tail_requires_layout && (target - current).abs() <= TAIL_POSITION_EPSILON {
        let mut state = scroll.borrow_mut();
        if state.pending == Some(request) {
            state.pending = None;
            if matches!(request, PreparedTemplatedScroll::Tail) {
                state.following_tail = true;
            } else if let PreparedTemplatedScroll::RestoreOffset { following_tail, .. } = request {
                state.following_tail = following_tail;
            }
        }
        return true;
    }

    let changed = viewer
        .ChangeViewWithOptionalAnimation(None, Some(target), None, true)
        .unwrap_or(false);
    if !changed {
        // `false` is not success: the view may not be laid out yet. Keep the
        // request armed for LayoutUpdated/the next realization pass.
        return false;
    }

    if wait_for_confirmation {
        // ViewChanged owns acknowledgement, because another layout pass can
        // increase ScrollableHeight while the change is being applied.
        return false;
    }

    let mut state = scroll.borrow_mut();
    if state.pending == Some(request) {
        state.pending = None;
        if let PreparedTemplatedScroll::RestoreOffset { following_tail, .. } = request {
            state.following_tail = following_tail;
        }
    }
    true
}

impl Default for WinUIBackend {
    fn default() -> Self {
        Self::new()
    }
}

/// Walks the visual tree from `root` down to find the first `IScrollViewer`.
///
/// `FrameworkElement.FindName` cannot reach template namescope elements, and
/// ListView's internal ScrollViewer lives in its template namescope — the
/// FindName approach silently fails even after the template is applied. A
/// VisualTreeHelper walk finds it by type instead.
fn find_templated_scroll_viewer(root: &bindings::DependencyObject) -> Option<bindings::IScrollViewer> {
    let count = bindings::VisualTreeHelper::GetChildrenCount(root).ok()?;
    for i in 0..count {
        let Ok(child) = bindings::VisualTreeHelper::GetChild(root, i) else {
            continue;
        };
        if let Ok(viewer) = child.cast::<bindings::IScrollViewer>() {
            return Some(viewer);
        }
        if let Some(viewer) = find_templated_scroll_viewer(&child) {
            return Some(viewer);
        }
    }
    None
}

impl WinUIBackend {
    pub fn new() -> Self {
        Self {
            controls: RefCell::new(FxHashMap::default()),
            event_revokers: RefCell::new(FxHashMap::default()),
            property_observers: RefCell::new(FxHashMap::default()),
            templated_selection_revokers: RefCell::new(FxHashMap::default()),
            templated: RefCell::new(FxHashMap::default()),
            content_template: RefCell::new(None),
            rich_text: RefCell::new(FxHashMap::default()),
            pointer_revokers: RefCell::new(FxHashMap::default()),
            drag_revokers: RefCell::new(FxHashMap::default()),
            menu_click_handlers: RefCell::new(FxHashMap::default()),
            command_bar_flyout_handlers: RefCell::new(FxHashMap::default()),
            theme_brush_registry: RefCell::new(FxHashMap::default()),
            resource_keys: RefCell::new(FxHashMap::default()),
            flyout_open_pending: RefCell::new(FxHashMap::default()),
            flyout_closed_pending: RefCell::new(FxHashMap::default()),
            elevation: RefCell::new(FxHashMap::default()),
            elevation_live: RefCell::new(FxHashSet::default()),
            dialog_state: Rc::new(RefCell::new(FxHashMap::default())),
            dialog_hide_pending: RefCell::new(FxHashMap::default()),
            window_state: RefCell::new(None),
            next_id: RefCell::new(0),
        }
    }

    pub(crate) fn set_window_state(&self, state: Rc<HostWindowState>) {
        *self.window_state.borrow_mut() = Some(state);
    }
    pub fn get_ui_element(&self, id: ControlId) -> Option<windows_core::IInspectable> {
        self.controls
            .borrow()
            .get(&id)
            .map(|h| h.as_ui_element().cast().unwrap())
    }
    /// Parses the shared virtualization item template on first use.
    fn content_template(&self) -> bindings::DataTemplate {
        if let Some(t) = self.content_template.borrow().as_ref() {
            return t.clone();
        }
        let template = bindings::XamlReader::Load(CONTENT_TEMPLATE_XAML)
            .unwrap()
            .cast::<bindings::DataTemplate>()
            .unwrap();
        *self.content_template.borrow_mut() = Some(template.clone());
        template
    }

    fn ensure_templated_scroll_viewer(&self, id: ControlId) -> bool {
        let (scroll, item_containers) = {
            let mut lists = self.templated.borrow_mut();
            let entry = lists.entry(id).or_insert_with(TemplatedList::new);
            if entry.shared.scroll.borrow().viewer.is_some() {
                return true;
            }
            (
                Rc::clone(&entry.shared.scroll),
                Rc::clone(&entry.shared.item_containers),
            )
        };

        let viewer = {
            let controls = self.controls.borrow();
            let Some(handle) = controls.get(&id) else {
                return false;
            };
            let Ok(fe) = handle.cast_inner::<bindings::IFrameworkElement>() else {
                return false;
            };
            let Ok(dep) = fe.cast::<bindings::DependencyObject>() else {
                return false;
            };
            match find_templated_scroll_viewer(&dep) {
                Some(viewer) => viewer,
                None => return false,
            }
        };

        {
            let mut state = scroll.borrow_mut();
            state.viewer = Some(viewer.clone());
            state.last_vertical_offset = viewer.VerticalOffset().unwrap_or(0.0);
        }
        let scroll_for_event = Rc::clone(&scroll);
        let viewer_for_event = viewer.clone();
        let Ok(revoker) = viewer.ViewChanged(move |_sender, _args| {
            let vertical = viewer_for_event.VerticalOffset().unwrap_or(0.0);
            let scrollable = viewer_for_event.ScrollableHeight().unwrap_or(0.0);
            let (top_callback, viewport_callback, viewport) = {
                let mut state = scroll_for_event.borrow_mut();
                observe_templated_view(&mut state, vertical, scrollable);
                let near_top = vertical <= state.top_threshold;
                let top_callback = (near_top && !state.near_top)
                    .then(|| state.on_top_reached.clone())
                    .flatten();
                state.near_top = near_top;
                let viewport = TemplatedViewport {
                    vertical_offset: vertical,
                    scrollable_height: scrollable,
                    following_tail: state.following_tail,
                };
                (top_callback, state.on_view_changed.clone(), viewport)
            };
            if let Some(callback) = top_callback {
                callback.invoke(());
            }
            if let Some(callback) = viewport_callback {
                callback.invoke(viewport);
            }
        }) else {
            scroll.borrow_mut().viewer = None;
            return false;
        };

        // Reconcile mutates the native tree before WinUI measure/arrange. A
        // persistent LayoutUpdated observer retries only while a request is
        // pending, using the final ScrollableHeight from that layout pass.
        let Ok(framework) = viewer.cast::<bindings::IFrameworkElement>() else {
            scroll.borrow_mut().viewer = None;
            return false;
        };
        let scroll_for_layout = Rc::clone(&scroll);
        let containers_for_layout = Rc::clone(&item_containers);
        let Ok(layout_revoker) = framework.LayoutUpdated(move |_sender, _args| {
            if scroll_for_layout.borrow().pending.is_some() {
                apply_prepared_templated_scroll_shared(&scroll_for_layout, &containers_for_layout);
            }
        }) else {
            scroll.borrow_mut().viewer = None;
            return false;
        };

        let mut lists = self.templated.borrow_mut();
        let entry = lists.entry(id).or_insert_with(TemplatedList::new);
        entry.view_changed_revoker = Some(revoker);
        entry.layout_updated_revoker = Some(layout_revoker);
        true
    }
    pub fn find_titlebar(&self) -> Option<bindings::TitleBar> {
        self.controls.borrow().values().find_map(|h| match h {
            Handle::TitleBar(tb) => Some(tb.clone()),
            _ => None,
        })
    }
    fn alloc_id(&self) -> ControlId {
        let mut counter = self.next_id.borrow_mut();
        *counter += 1;
        ControlId::new(*counter)
    }

    /// Applies open/close requests and Closed handlers that arrived before
    /// the flyout was created (props/events attach before the content slot
    /// mounts). Called when the `Flyout` comes into existence.
    fn consume_flyout_pending(&self, id: ControlId, b: &bindings::IButton) -> Result<()> {
        let flyout = b.Flyout()?;
        let fb = flyout.cast::<bindings::IFlyoutBase>()?;
        if let Some(open) = self.flyout_open_pending.borrow_mut().remove(&id) {
            if open {
                let target = b.cast::<bindings::FrameworkElement>()?;
                fb.ShowAt(&target)?;
            } else {
                fb.Hide()?;
            }
        }
        if let Some(handler) = self.flyout_closed_pending.borrow_mut().remove(&id) {
            let revoker = fb.Closed(move |_, _| handler.invoke())?;
            self.event_revokers
                .borrow_mut()
                .entry((id, Event::FlyoutClosed))
                .or_default()
                .push(revoker);
        }
        Ok(())
    }

    fn observe_navigation_state(
        &self,
        id: ControlId,
        event: Event,
        navigation: &bindings::NavigationView,
        handler: EventHandler,
    ) -> Result<()> {
        let property = match event {
            Event::NavigationPaneOpenChanged => bindings::NavigationView::IsPaneOpenProperty()?,
            Event::NavigationDisplayModeChanged => bindings::NavigationView::DisplayModeProperty()?,
            _ => unreachable!(),
        };
        let object = navigation.cast::<bindings::DependencyObject>()?;
        let navigation = navigation.clone();
        let callback = bindings::DependencyPropertyChangedCallback::new(
            move |_sender, _property| match event {
                Event::NavigationPaneOpenChanged => match navigation.IsPaneOpen() {
                    Ok(open) => handler.invoke_bool(open),
                    Err(error) => diag::warn(format_args!(
                        "failed to read NavigationView.IsPaneOpen for {id}: {error:?}"
                    )),
                },
                Event::NavigationDisplayModeChanged => match navigation.DisplayMode() {
                    Ok(mode) => handler.invoke_navigation_display_mode(mode),
                    Err(error) => diag::warn(format_args!(
                        "failed to read NavigationView.DisplayMode for {id}: {error:?}"
                    )),
                },
                _ => unreachable!(),
            },
        );
        let token = object.RegisterPropertyChangedCallback(&property, &callback)?;
        self.property_observers.borrow_mut().insert(
            (id, event),
            PropertyObserver {
                object,
                property,
                token,
            },
        );
        Ok(())
    }

    fn set_resources(
        &self,
        id: ControlId,
        handle: &Handle,
        resources: &HashMap<String, ResourceValue>,
    ) -> Result<()> {
        let dictionary = handle.as_framework_element().Resources()?;
        let map = dictionary.cast::<windows_collections::IMap<
            windows_core::IInspectable,
            windows_core::IInspectable,
        >>()?;

        let previous = self
            .resource_keys
            .borrow()
            .get(&id)
            .cloned()
            .unwrap_or_default();
        for key in previous {
            if resources.contains_key(&key) {
                continue;
            }
            let key = windows_reference::IReference::from(key.as_str());
            if map.HasKey(&key)? {
                map.Remove(&key)?;
            }
        }

        for (key, value) in resources {
            let key = windows_reference::IReference::from(key.as_str());
            let value: windows_core::IInspectable = match value {
                ResourceValue::String(value) => {
                    windows_reference::IReference::from(value.as_str()).cast()?
                }
                ResourceValue::SolidColorBrush(color) => solid_brush(*color)?.cast()?,
                ResourceValue::F64(value) => windows_reference::IReference::from(*value).cast()?,
                ResourceValue::Thickness(value) => {
                    windows_reference::IReference::from(*value).cast()?
                }
                ResourceValue::CornerRadius(value) => {
                    windows_reference::IReference::from(bindings::CornerRadius {
                        top_left: value.top_left,
                        top_right: value.top_right,
                        bottom_right: value.bottom_right,
                        bottom_left: value.bottom_left,
                    })
                    .cast()?
                }
            };
            map.Insert(&key, &value)?;
        }

        let mut resource_keys = self.resource_keys.borrow_mut();
        if resources.is_empty() {
            resource_keys.remove(&id);
        } else {
            resource_keys.insert(id, resources.keys().cloned().collect::<FxHashSet<_>>());
        }
        Ok(())
    }
    fn wire_menu_bar_clicks(
        mb: &bindings::MenuBar,
        handler: &EventHandler,
    ) -> Vec<EventRevoker> {
        let mut revokers = Vec::new();
        let Ok(bar_items) = mb.Items() else {
            return revokers;
        };
        for mbi in &bar_items {
            if let Ok(flyout_items) = mbi.Items() {
                Self::wire_flyout_items_click(&flyout_items, handler, &mut revokers);
            }
        }
        revokers
    }

    fn wire_flyout_clicks(
        flyout: &bindings::MenuFlyout,
        handler: &EventHandler,
    ) -> Vec<EventRevoker> {
        let mut revokers = Vec::new();
        if let Ok(items) = flyout.Items() {
            Self::wire_flyout_items_click(&items, handler, &mut revokers);
        }
        revokers
    }

    fn wire_flyout_items_click(
        items: &windows_collections::IVector<bindings::MenuFlyoutItemBase>,
        handler: &EventHandler,
        revokers: &mut Vec<EventRevoker>,
    ) {
        for base in items {
            if let Ok(item) = base.cast::<bindings::MenuFlyoutItem>() {
                let text = item.Text().unwrap_or_default().clone();
                let handler = handler.clone();
                if let Ok(rev) = item.Click(move |_s, _a| {
                    handler.invoke_string(text.clone());
                }) {
                    revokers.push(rev);
                }
            } else if let Ok(sub) = base.cast::<bindings::MenuFlyoutSubItem>()
                && let Ok(sub_items) = sub.Items()
            {
                Self::wire_flyout_items_click(&sub_items, handler, revokers);
            }
        }
    }

    fn wire_command_bar_clicks(
        commands: &windows_collections::IObservableVector<bindings::ICommandBarElement>,
        handler: &EventHandler,
    ) -> Vec<EventRevoker> {
        let mut revokers = Vec::new();
        for el in commands {
            if let Ok(btn) = el.cast::<bindings::AppBarButton>() {
                let label = btn.Label().unwrap_or_default().clone();
                let handler = handler.clone();
                if let Ok(rev) = btn.cast::<bindings::ButtonBase>().and_then(|bb| {
                    bb.Click(move |_s, _a| {
                        handler.invoke_string(label.clone());
                    })
                }) {
                    revokers.push(rev);
                }
            }
        }
        revokers
    }
}

enum ContainerChildren<'a> {
    Panel(bindings::UIElementCollection),
    SingleChild(&'a Handle),
    ContentControl(bindings::IContentControl),
    DirectContent(&'a Handle),
    InspectableVector(windows_collections::IVector<windows_core::IInspectable>),
}

fn classify_container(h: &Handle) -> Option<ContainerChildren<'_>> {
    match h {
        Handle::StackPanel(s) => Some(ContainerChildren::Panel(
            s.cast::<bindings::IPanel>().ok()?.Children().ok()?,
        )),
        Handle::Grid(g) => Some(ContainerChildren::Panel(
            g.cast::<bindings::IPanel>().ok()?.Children().ok()?,
        )),
        Handle::Canvas(c) => Some(ContainerChildren::Panel(
            c.cast::<bindings::IPanel>().ok()?.Children().ok()?,
        )),
        Handle::RelativePanel(r) => Some(ContainerChildren::Panel(
            r.cast::<bindings::IPanel>().ok()?.Children().ok()?,
        )),
        Handle::Border(_) | Handle::Viewbox(_) => Some(ContainerChildren::SingleChild(h)),
        Handle::ScrollViewer(s) => Some(ContainerChildren::ContentControl(s.cast().ok()?)),
        Handle::Expander(e) => Some(ContainerChildren::ContentControl(e.cast().ok()?)),
        Handle::TabViewItem(ti) => Some(ContainerChildren::ContentControl(ti.cast().ok()?)),
        Handle::NavigationView(nv) => Some(ContainerChildren::ContentControl(nv.cast().ok()?)),
        Handle::PivotItem(pi) => Some(ContainerChildren::ContentControl(pi.cast().ok()?)),
        Handle::ScrollView(_) | Handle::SplitView(_) => Some(ContainerChildren::DirectContent(h)),
        Handle::TabView(tv) => Some(ContainerChildren::InspectableVector(tv.TabItems().ok()?)),
        Handle::Pivot(p) => Some(ContainerChildren::InspectableVector(
            p.cast::<bindings::IItemsControl>()
                .ok()?
                .Items()
                .ok()?
                .cast()
                .ok()?,
        )),
        _ => None,
    }
}

fn container_append(cc: &ContainerChildren<'_>, child: &bindings::UIElement) {
    match cc {
        ContainerChildren::Panel(vec) => vec.Append(child).unwrap(),
        ContainerChildren::SingleChild(h) => put_single_child(h, Some(child)),
        ContainerChildren::ContentControl(c) => c.SetContent(child).unwrap(),
        ContainerChildren::DirectContent(h) => put_direct_content(h, Some(child)),
        ContainerChildren::InspectableVector(vec) => {
            let insp: windows_core::IInspectable = child.cast().unwrap();
            vec.Append(&insp).unwrap();
        }
    }
}

fn container_insert(cc: &ContainerChildren<'_>, index: usize, child: &bindings::UIElement) {
    match cc {
        ContainerChildren::Panel(vec) => vec.InsertAt(index as u32, child).unwrap(),
        ContainerChildren::SingleChild(h) => put_single_child(h, Some(child)),
        ContainerChildren::ContentControl(c) => c.SetContent(child).unwrap(),
        ContainerChildren::DirectContent(h) => put_direct_content(h, Some(child)),
        ContainerChildren::InspectableVector(vec) => {
            let insp: windows_core::IInspectable = child.cast().unwrap();
            vec.InsertAt(index as u32, &insp).unwrap();
        }
    }
}

fn container_set(cc: &ContainerChildren<'_>, index: usize, child: &bindings::UIElement) {
    match cc {
        ContainerChildren::Panel(vec) => vec.SetAt(index as u32, child).unwrap(),
        ContainerChildren::SingleChild(h) => put_single_child(h, Some(child)),
        ContainerChildren::ContentControl(c) => c.SetContent(child).unwrap(),
        ContainerChildren::DirectContent(h) => put_direct_content(h, Some(child)),
        ContainerChildren::InspectableVector(vec) => {
            let insp: windows_core::IInspectable = child.cast().unwrap();
            vec.SetAt(index as u32, &insp).unwrap();
        }
    }
}

fn container_remove(cc: &ContainerChildren<'_>, index: usize) {
    match cc {
        ContainerChildren::Panel(vec) => vec.RemoveAt(index as u32).unwrap(),
        ContainerChildren::SingleChild(h) => {
            debug_assert_eq!(index, 0);
            put_single_child(h, None);
        }
        ContainerChildren::ContentControl(c) => {
            debug_assert_eq!(index, 0);
            c.SetContent(None::<&windows_core::IInspectable>).unwrap();
        }
        ContainerChildren::DirectContent(h) => {
            debug_assert_eq!(index, 0);
            put_direct_content(h, None);
        }
        ContainerChildren::InspectableVector(vec) => vec.RemoveAt(index as u32).unwrap(),
    }
}

fn container_move(cc: &ContainerChildren<'_>, from: usize, to: usize) {
    match cc {
        ContainerChildren::Panel(vec) => {
            let item = vec.GetAt(from as u32).unwrap();
            vec.RemoveAt(from as u32).unwrap();
            vec.InsertAt(to as u32, &item).unwrap();
        }
        ContainerChildren::SingleChild(_)
        | ContainerChildren::ContentControl(_)
        | ContainerChildren::DirectContent(_) => {}
        ContainerChildren::InspectableVector(vec) => {
            let item = vec.GetAt(from as u32).unwrap();
            vec.RemoveAt(from as u32).unwrap();
            vec.InsertAt(to as u32, &item).unwrap();
        }
    }
}

fn put_single_child(h: &Handle, child: Option<&bindings::UIElement>) {
    match h {
        Handle::Border(b) => b.SetChild(child).unwrap(),
        Handle::Viewbox(v) => v.SetChild(child).unwrap(),
        _ => unreachable!(),
    }
}

fn put_direct_content(h: &Handle, child: Option<&bindings::UIElement>) {
    match h {
        Handle::ScrollView(sv) => sv.SetContent(child).unwrap(),
        Handle::SplitView(sv) => sv.SetContent(child).unwrap(),
        _ => unreachable!(),
    }
}

fn apply_theme_resource_style(handle: &Handle, bindings: &[(Prop, ThemeRef)]) {
    let Some((target_type, fe)) = style_target_for_handle(handle) else {
        return;
    };

    let mut setters = String::new();
    for (prop, theme_ref) in bindings {
        let dp_name = match prop {
            Prop::Background => "Background",
            Prop::Foreground => "Foreground",
            Prop::BorderBrush => "BorderBrush",
            _ => continue,
        };
        let resource_key = theme_ref.resource_key();
        setters.push_str(&format!(
            "<Setter Property='{dp_name}' Value='{{ThemeResource {resource_key}}}'/>"
        ));
    }

    if setters.is_empty() {
        diag::dropped(fe.SetStyle(None));
        return;
    }

    let xaml = format!(
        "<Style xmlns='http://schemas.microsoft.com/winfx/2006/xaml/presentation' TargetType='{target_type}'>{setters}</Style>"
    );

    match bindings::XamlReader::Load(&xaml) {
        Ok(obj) => {
            if let Ok(style) = obj.cast::<bindings::Style>() {
                // Force WinUI to re-resolve {ThemeResource} values.
                diag::dropped(fe.SetStyle(None));
                diag::dropped(fe.SetStyle(&style));
            }
        }
        Err(e) => {
            diag::warn(format_args!(
                "ThemeStyle: XamlReader::Load failed: {e:?} xaml={xaml}"
            ));
        }
    }
}

fn style_target_for_handle(handle: &Handle) -> Option<(&'static str, bindings::IFrameworkElement)> {
    match handle {
        Handle::Border(b) => b.cast().ok().map(|fe| ("Border", fe)),
        Handle::StackPanel(s) => s.cast().ok().map(|fe| ("StackPanel", fe)),
        Handle::Grid(g) => g.cast().ok().map(|fe| ("Grid", fe)),
        Handle::Button(b) => b.cast().ok().map(|fe| ("Button", fe)),
        Handle::TextBox(t) => t.cast().ok().map(|fe| ("TextBox", fe)),
        Handle::TextBlock(t) => t.cast().ok().map(|fe| ("TextBlock", fe)),
        Handle::Canvas(c) => c.cast().ok().map(|fe| ("Canvas", fe)),
        _ => None,
    }
}

// Composition animations use the element backing visual from ElementCompositionPreview.

fn easing_for(
    compositor: &windows_composition::Compositor,
    easing: Easing,
) -> windows_composition::CompositionEasingFunction {
    let (p1, p2) = match easing {
        Easing::Linear => return compositor.create_linear_easing_function(),
        Easing::EaseOut => (
            windows_numerics::Vector2 { x: 0.0, y: 0.0 },
            windows_numerics::Vector2 { x: 0.58, y: 1.0 },
        ),
        Easing::EaseIn => (
            windows_numerics::Vector2 { x: 0.42, y: 0.0 },
            windows_numerics::Vector2 { x: 1.0, y: 1.0 },
        ),
        Easing::EaseInOut => (
            windows_numerics::Vector2 { x: 0.42, y: 0.0 },
            windows_numerics::Vector2 { x: 0.58, y: 1.0 },
        ),
    };
    compositor.create_cubic_bezier_easing_function(p1, p2)
}

fn element_visual(ui: &bindings::UIElement) -> Result<windows_composition::Visual> {
    let raw = bindings::ElementCompositionPreview::GetElementVisual(ui)?;
    windows_composition::Visual::from_host(raw.into())
}

fn apply_implicit_transitions(
    ui: &bindings::UIElement,
    transitions: Option<ImplicitTransitions>,
) -> Result<()> {
    let visual = element_visual(ui)?;
    let Some(t) = transitions.filter(|t| !t.is_empty()) else {
        visual.set_implicit_animations(None);
        return Ok(());
    };
    let compositor = visual.compositor();
    let collection = compositor.create_implicit_animation_collection();

    // The DSL exposes duration only, so implicit transitions use XAML's EaseOut curve.
    let insert = |target: &str, duration: std::time::Duration, is_scalar: bool| {
        let easing = easing_for(&compositor, Easing::EaseOut);
        if is_scalar {
            let a = compositor.create_scalar_key_frame_animation();
            a.set_duration(duration);
            a.insert_expression_key_frame_with_easing(1.0, "this.FinalValue", &easing);
            a.set_target(target);
            collection.insert(target, &a);
        } else {
            let a = compositor.create_vector3_key_frame_animation();
            a.set_duration(duration);
            a.insert_expression_key_frame_with_easing(1.0, "this.FinalValue", &easing);
            a.set_target(target);
            collection.insert(target, &a);
        }
    };

    if let Some(s) = t.opacity {
        insert("Opacity", s.duration, true);
    }
    if let Some(s) = t.rotation {
        insert("RotationAngleInDegrees", s.duration, true);
    }
    if let Some(v) = t.scale {
        insert("Scale", v.duration, false);
    }
    if let Some(v) = t.translation {
        // `Offset` collides with XAML layout; this should target `Translation`.
        insert("Offset", v.duration, false);
    }
    visual.set_implicit_animations(Some(&collection));
    Ok(())
}

/// Layout-driven implicit animation (F-N11): whenever XAML arrange changes
/// the backing visual's Size/Offset, tween from the previous value to the
/// new one. Owns the element's ImplicitAnimations collection entirely
/// (enter/exit use the separate Show/Hide implicit APIs and are unaffected;
/// combining with the ImplicitTransitions DSL on one element is unsupported
/// in v1). Spring curves need NaturalMotionAnimation wrappers — follow-up.
fn apply_layout_animation(
    ui: &bindings::UIElement,
    config: Option<LayoutAnimationConfig>,
) -> Result<()> {
    let visual = element_visual(ui)?;
    let compositor = visual.compositor();
    // v1 独占语义：布局动画整体接管元素的 ImplicitAnimations 集合。
    let collection = compositor.create_implicit_animation_collection();
    let Some(c) = config else {
        // 清除布局动画 = 换成空集合（Size/Offset 键随集合消失）。
        visual.set_implicit_animations(None);
        return Ok(());
    };
    let insert_tween = |target: &str| {
        collection.remove(target);
        let a = compositor.create_vector3_key_frame_animation();
        a.set_duration(c.duration);
        let easing = easing_for(&compositor, Easing::EaseOut);
        a.insert_expression_key_frame_with_easing(1.0, "this.FinalValue", &easing);
        a.set_target(target);
        collection.insert(target, &a);
    };
    if c.animate_offset {
        insert_tween("Offset");
    }
    if c.animate_size {
        // Visual.Size 是 Vector2：必须用 vector2 关键帧动画
        //（vector3 喂 Vector2 属性会 E_INVALIDARG → stowed crash）。
        collection.remove("Size");
        let a = compositor.create_vector2_key_frame_animation();
        a.set_duration(c.duration);
        let easing = easing_for(&compositor, Easing::EaseOut);
        a.insert_expression_key_frame_with_easing(1.0, "this.FinalValue", &easing);
        a.set_target("Size");
        collection.insert("Size", &a);
    }
    visual.set_implicit_animations(Some(&collection));
    Ok(())
}

fn run_property_animation_inner(ui: &bindings::UIElement, cfg: AnimationConfig) -> Result<()> {
    let visual = element_visual(ui)?;
    let compositor = visual.compositor();

    if let Some(opacity) = cfg.opacity {
        let a = compositor.create_scalar_key_frame_animation();
        a.set_duration(cfg.duration);
        let easing = easing_for(&compositor, cfg.easing);
        a.insert_key_frame_with_easing(1.0, opacity as f32, &easing);
        visual.start_animation("Opacity", &a);
    }
    if let Some(scale) = cfg.scale {
        let current = visual.scale();
        let s = scale as f32;
        if current.x == s && current.y == s {
            return Ok(());
        }
        // Before first layout, ActualWidth/Height are 0 and CenterPoint is reused.
        if let Ok(fe) = ui.cast::<bindings::IFrameworkElement>() {
            let w = fe.ActualWidth().unwrap_or(0.0) as f32;
            let h = fe.ActualHeight().unwrap_or(0.0) as f32;
            if w > 0.0 && h > 0.0 {
                visual.set_center_point(windows_numerics::Vector3 {
                    x: w / 2.0,
                    y: h / 2.0,
                    z: 0.0,
                });
            } else {
                diag::warn(format_args!(
                    "animation: skipping CenterPoint - element not yet laid out"
                ));
            }
        }
        let a = compositor.create_vector3_key_frame_animation();
        a.set_duration(cfg.duration);
        let easing = easing_for(&compositor, cfg.easing);
        a.insert_key_frame_with_easing(
            1.0,
            windows_numerics::Vector3 {
                x: s,
                y: s,
                z: current.z,
            },
            &easing,
        );
        visual.start_animation("Scale", &a);
    }
    if let Some(t) = cfg.translation {
        // 一次性位移动画：target Offset（Composition Visual 可动画属性）。
        let a = compositor.create_vector3_key_frame_animation();
        a.set_duration(cfg.duration);
        let easing = easing_for(&compositor, cfg.easing);
        a.insert_key_frame_with_easing(1.0, t, &easing);
        visual.start_animation("Offset", &a);
    }
    Ok(())
}

fn build_element_transition_animation(
    ui: &bindings::UIElement,
    cfg: AnimationConfig,
    is_enter: bool,
) -> Result<Option<bindings::ICompositionAnimationBase>> {
    if cfg.opacity.is_none() && cfg.scale.is_none() {
        return Ok(None);
    }

    let visual = element_visual(ui)?;
    let compositor = visual.compositor();
    let easing = easing_for(&compositor, cfg.easing);
    let group = compositor.create_animation_group();

    if let Some(opacity) = cfg.opacity {
        let animation = compositor.create_scalar_key_frame_animation();
        animation.set_duration(cfg.duration);
        animation.set_target("Opacity");
        if is_enter {
            animation.insert_key_frame_with_easing(0.0, 0.0, &easing);
        }
        animation.insert_key_frame_with_easing(1.0, opacity as f32, &easing);
        group.add(&animation);
    }

    if let Some(scale) = cfg.scale {
        let z = visual.scale().z;
        let animation = compositor.create_vector3_key_frame_animation();
        animation.set_duration(cfg.duration);
        animation.set_target("Scale");
        if is_enter {
            animation.insert_key_frame_with_easing(
                0.0,
                windows_numerics::Vector3 { x: 0.0, y: 0.0, z },
                &easing,
            );
        }
        let scale = scale as f32;
        animation.insert_key_frame_with_easing(
            1.0,
            windows_numerics::Vector3 {
                x: scale,
                y: scale,
                z,
            },
            &easing,
        );
        group.add(&animation);
    }

    if let Some(t) = cfg.translation {
        // 动画 target 必须是 Composition Visual 的可动画属性：
        // Translation 不存在（XAML UIElement 才有）——用 Offset（Vector3）。
        // 入场动画在布局完成后触发，动画结束 Offset 回到布局值，无残留。
        let animation = compositor.create_vector3_key_frame_animation();
        animation.set_duration(cfg.duration);
        animation.set_target("Offset");
        let zero = windows_numerics::Vector3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        if is_enter {
            animation.insert_key_frame_with_easing(0.0, t, &easing);
        }
        animation.insert_key_frame_with_easing(1.0, zero, &easing);
        group.add(&animation);
    }

    Ok(Some(group.as_host().cast()?))
}

fn apply_element_transitions(
    ui: &bindings::UIElement,
    enter: Option<AnimationConfig>,
    exit: Option<AnimationConfig>,
) -> Result<()> {
    let enter = enter
        .map(|config| build_element_transition_animation(ui, config, true))
        .transpose()?
        .flatten();
    let exit = exit
        .map(|config| build_element_transition_animation(ui, config, false))
        .transpose()?
        .flatten();

    bindings::ElementCompositionPreview::SetImplicitShowAnimation(ui, enter.as_ref())?;
    bindings::ElementCompositionPreview::SetImplicitHideAnimation(ui, exit.as_ref())
}

/// Handles props shared by base-class interfaces.
fn try_universal_prop(handle: &Handle, prop: Prop, value: &PropValue) -> Result<bool> {
    match (prop, value) {
        (Prop::FontSize, PropValue::F64(v)) => set_font_f64(handle, *v),
        (Prop::FontSize, PropValue::Unset) => set_font_f64(handle, 14.0),
        (Prop::FontWeight, PropValue::U16(w)) => {
            set_font_weight(handle, bindings::FontWeight { weight: *w })
        }
        (Prop::FontWeight, PropValue::Unset) => {
            set_font_weight(handle, bindings::FontWeight { weight: 400 })
        }
        (Prop::FontFamily, PropValue::Str(s)) => {
            set_font_family(handle, &bindings::FontFamily::CreateInstanceWithName(s)?)
        }
        (Prop::FontFamily, PropValue::Unset) => set_font_family(
            handle,
            &bindings::FontFamily::CreateInstanceWithName("Segoe UI")?,
        ),
        (Prop::Margin, PropValue::Thickness(t)) => {
            handle.as_framework_element().SetMargin(*t)?;
            Ok(true)
        }
        (Prop::Margin, PropValue::Unset) => {
            handle
                .as_framework_element()
                .SetMargin(Thickness::default())?;
            Ok(true)
        }
        (Prop::Width, PropValue::F64(v)) => {
            handle.as_framework_element().SetWidth(*v)?;
            Ok(true)
        }
        (Prop::Width, PropValue::Unset) => {
            handle.as_framework_element().SetWidth(f64::NAN)?;
            Ok(true)
        }
        (Prop::Height, PropValue::F64(v)) => {
            handle.as_framework_element().SetHeight(*v)?;
            Ok(true)
        }
        (Prop::Height, PropValue::Unset) => {
            handle.as_framework_element().SetHeight(f64::NAN)?;
            Ok(true)
        }
        (Prop::MinWidth, PropValue::F64(v)) => {
            handle.as_framework_element().SetMinWidth(*v)?;
            Ok(true)
        }
        (Prop::MinWidth, PropValue::Unset) => {
            handle.as_framework_element().SetMinWidth(0.0)?;
            Ok(true)
        }
        (Prop::MaxWidth, PropValue::F64(v)) => {
            handle.as_framework_element().SetMaxWidth(*v)?;
            Ok(true)
        }
        (Prop::MaxWidth, PropValue::Unset) => {
            handle.as_framework_element().SetMaxWidth(f64::INFINITY)?;
            Ok(true)
        }
        (Prop::MinHeight, PropValue::F64(v)) => {
            handle.as_framework_element().SetMinHeight(*v)?;
            Ok(true)
        }
        (Prop::MinHeight, PropValue::Unset) => {
            handle.as_framework_element().SetMinHeight(0.0)?;
            Ok(true)
        }
        (Prop::MaxHeight, PropValue::F64(v)) => {
            handle.as_framework_element().SetMaxHeight(*v)?;
            Ok(true)
        }
        (Prop::MaxHeight, PropValue::Unset) => {
            handle.as_framework_element().SetMaxHeight(f64::INFINITY)?;
            Ok(true)
        }
        (Prop::HorizontalAlignment, PropValue::I32(v)) => {
            handle
                .as_framework_element()
                .SetHorizontalAlignment(HorizontalAlignment(*v))?;
            Ok(true)
        }
        (Prop::HorizontalAlignment, PropValue::Unset) => {
            handle
                .as_framework_element()
                .SetHorizontalAlignment(HorizontalAlignment::Stretch)?;
            Ok(true)
        }
        (Prop::VerticalAlignment, PropValue::I32(v)) => {
            handle
                .as_framework_element()
                .SetVerticalAlignment(VerticalAlignment(*v))?;
            Ok(true)
        }
        (Prop::VerticalAlignment, PropValue::Unset) => {
            handle
                .as_framework_element()
                .SetVerticalAlignment(VerticalAlignment::Stretch)?;
            Ok(true)
        }
        (Prop::Opacity, PropValue::F64(v)) => {
            handle.as_ui_element().SetOpacity(*v)?;
            Ok(true)
        }
        (Prop::Opacity, PropValue::Unset) => {
            handle.as_ui_element().SetOpacity(1.0)?;
            Ok(true)
        }
        (Prop::AllowDrop, PropValue::Bool(v)) => {
            handle.as_ui_element().SetAllowDrop(*v)?;
            Ok(true)
        }
        (Prop::AllowDrop, PropValue::Unset) => {
            handle.as_ui_element().SetAllowDrop(false)?;
            Ok(true)
        }
        (Prop::IsEnabled, PropValue::Unset) => {
            handle
                .as_ui_element()
                .cast::<bindings::IControl>()?
                .SetIsEnabled(true)?;
            Ok(true)
        }
        (Prop::AttachedGridRow, PropValue::I32(v)) => {
            bindings::Grid::SetRow(&handle.as_framework_element(), *v)?;
            Ok(true)
        }
        (Prop::AttachedGridColumn, PropValue::I32(v)) => {
            bindings::Grid::SetColumn(&handle.as_framework_element(), *v)?;
            Ok(true)
        }
        (Prop::AttachedGridRowSpan, PropValue::I32(v)) => {
            bindings::Grid::SetRowSpan(&handle.as_framework_element(), *v)?;
            Ok(true)
        }
        (Prop::AttachedGridColumnSpan, PropValue::I32(v)) => {
            bindings::Grid::SetColumnSpan(&handle.as_framework_element(), *v)?;
            Ok(true)
        }
        (Prop::AttachedCanvasLeft, PropValue::F64(v)) => {
            bindings::Canvas::SetLeft(&handle.as_ui_element(), *v)?;
            Ok(true)
        }
        (Prop::AttachedCanvasTop, PropValue::F64(v)) => {
            bindings::Canvas::SetTop(&handle.as_ui_element(), *v)?;
            Ok(true)
        }
        (Prop::AttachedCanvasZIndex, PropValue::I32(v)) => {
            bindings::Canvas::SetZIndex(&handle.as_ui_element(), *v)?;
            Ok(true)
        }
        (Prop::AlignLeftWithPanel, PropValue::Bool(v)) => {
            bindings::RelativePanel::SetAlignLeftWithPanel(&handle.as_ui_element(), *v)?;
            Ok(true)
        }
        (Prop::AlignRightWithPanel, PropValue::Bool(v)) => {
            bindings::RelativePanel::SetAlignRightWithPanel(&handle.as_ui_element(), *v)?;
            Ok(true)
        }
        (Prop::AlignTopWithPanel, PropValue::Bool(v)) => {
            bindings::RelativePanel::SetAlignTopWithPanel(&handle.as_ui_element(), *v)?;
            Ok(true)
        }
        (Prop::AlignBottomWithPanel, PropValue::Bool(v)) => {
            bindings::RelativePanel::SetAlignBottomWithPanel(&handle.as_ui_element(), *v)?;
            Ok(true)
        }
        (Prop::AlignHCenterWithPanel, PropValue::Bool(v)) => {
            bindings::RelativePanel::SetAlignHorizontalCenterWithPanel(
                &handle.as_ui_element(),
                *v,
            )?;
            Ok(true)
        }
        (Prop::AlignVCenterWithPanel, PropValue::Bool(v)) => {
            bindings::RelativePanel::SetAlignVerticalCenterWithPanel(&handle.as_ui_element(), *v)?;
            Ok(true)
        }
        (Prop::Padding, PropValue::Thickness(t)) => set_padding(handle, *t),
        (Prop::Padding, PropValue::Unset) => set_padding(handle, Thickness::default()),
        (Prop::Background, PropValue::Color(br)) => set_background(handle, &solid_brush(*br)?),
        (Prop::Background, PropValue::Unset) => set_background(handle, None::<&bindings::Brush>),
        (Prop::Foreground, PropValue::Color(br)) => set_foreground(handle, &solid_brush(*br)?),
        (Prop::Foreground, PropValue::Gradient(g)) => {
            set_foreground(handle, &gradient_brush(g)?)
        }
        (Prop::Foreground, PropValue::Unset) => set_foreground(handle, None::<&bindings::Brush>),
        (Prop::Fill, PropValue::Color(b)) => {
            handle
                .cast_inner::<bindings::IShape>()?
                .SetFill(&solid_brush(*b)?)?;
            Ok(true)
        }
        (Prop::Fill, PropValue::Unset) => {
            handle.cast_inner::<bindings::IShape>()?.SetFill(None)?;
            Ok(true)
        }
        (Prop::Stroke, PropValue::Color(b)) => {
            handle
                .cast_inner::<bindings::IShape>()?
                .SetStroke(&solid_brush(*b)?)?;
            Ok(true)
        }
        (Prop::Stroke, PropValue::Unset) => {
            handle.cast_inner::<bindings::IShape>()?.SetStroke(None)?;
            Ok(true)
        }
        (Prop::StrokeThickness, PropValue::F64(v)) => {
            handle
                .cast_inner::<bindings::IShape>()?
                .SetStrokeThickness(*v)?;
            Ok(true)
        }
        (Prop::StrokeThickness, PropValue::Unset) => {
            handle
                .cast_inner::<bindings::IShape>()?
                .SetStrokeThickness(0.0)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn set_padding(handle: &Handle, thickness: Thickness) -> Result<bool> {
    match handle {
        Handle::Border(h) => h.SetPadding(thickness)?,
        Handle::StackPanel(h) => h.SetPadding(thickness)?,
        Handle::TextBlock(h) => h.SetPadding(thickness)?,
        Handle::RichTextBlock(h) => h.SetPadding(thickness)?,
        // `Grid` is a `Panel`, not a `Control`, so it has no `IControl::SetPadding`;
        // its padding lives on the `IGrid` interface instead.
        Handle::Grid(h) => h.cast::<bindings::IGrid>()?.SetPadding(thickness)?,
        Handle::SwapChainPanel(h) => h.cast::<bindings::IGrid>()?.SetPadding(thickness)?,
        _ => {
            if let Ok(ctl) = handle.cast_inner::<bindings::IControl>() {
                ctl.SetPadding(thickness)?;
            } else {
                diag::unhandled_modifier("set_prop", Prop::Padding, handle);
            }
        }
    }
    Ok(true)
}

fn set_background(
    handle: &Handle,
    brush: impl windows_core::Param<bindings::Brush>,
) -> Result<bool> {
    match handle {
        Handle::Border(b) => b.SetBackground(brush)?,
        _ => {
            if let Ok(panel) = handle.cast_inner::<bindings::IPanel>() {
                panel.SetBackground(brush)?;
            } else if let Ok(ctl) = handle.cast_inner::<bindings::IControl>() {
                ctl.SetBackground(brush)?;
            } else {
                diag::unhandled_modifier("set_prop", Prop::Background, handle);
            }
        }
    }
    Ok(true)
}

fn set_foreground(
    handle: &Handle,
    brush: impl windows_core::Param<bindings::Brush>,
) -> Result<bool> {
    match handle {
        Handle::TextBlock(h) => h.SetForeground(brush)?,
        Handle::RichTextBlock(h) => h.SetForeground(brush)?,
        _ => {
            if let Ok(ctl) = handle.cast_inner::<bindings::IControl>() {
                ctl.SetForeground(brush)?;
            } else {
                diag::unhandled_modifier("set_prop", Prop::Foreground, handle);
            }
        }
    }
    Ok(true)
}

fn set_border_brush(
    handle: &Handle,
    brush: impl windows_core::Param<bindings::Brush>,
) -> Result<()> {
    match handle {
        Handle::Border(b) => b.SetBorderBrush(brush)?,
        _ => {
            if let Ok(ctl) = handle.cast_inner::<bindings::IControl>() {
                ctl.SetBorderBrush(brush)?;
            } else {
                diag::unhandled_modifier("set_prop", Prop::BorderBrush, handle);
            }
        }
    }
    Ok(())
}

fn set_border_thickness(handle: &Handle, thickness: Thickness) -> Result<()> {
    match handle {
        Handle::Border(b) => b.SetBorderThickness(thickness)?,
        _ => {
            if let Ok(ctl) = handle.cast_inner::<bindings::IControl>() {
                ctl.SetBorderThickness(thickness)?;
            } else {
                diag::unhandled_modifier("set_prop", Prop::BorderThickness, handle);
            }
        }
    }
    Ok(())
}

fn set_font_f64(handle: &Handle, v: f64) -> Result<bool> {
    match handle {
        Handle::TextBlock(h) => h.SetFontSize(v)?,
        Handle::RichTextBlock(h) => h.SetFontSize(v)?,
        _ => {
            if let Ok(ctl) = handle.cast_inner::<bindings::IControl>() {
                ctl.SetFontSize(v)?;
            } else {
                diag::unhandled_modifier("set_prop", Prop::FontSize, handle);
            }
        }
    }
    Ok(true)
}

fn set_font_weight(handle: &Handle, fw: bindings::FontWeight) -> Result<bool> {
    match handle {
        Handle::TextBlock(h) => h.SetFontWeight(fw)?,
        _ => {
            if let Ok(ctl) = handle.cast_inner::<bindings::IControl>() {
                ctl.SetFontWeight(fw)?;
            } else {
                diag::unhandled_modifier("set_prop", Prop::FontWeight, handle);
            }
        }
    }
    Ok(true)
}

fn set_font_family(handle: &Handle, ff: &bindings::FontFamily) -> Result<bool> {
    match handle {
        Handle::TextBlock(h) => h.SetFontFamily(ff)?,
        Handle::RichTextBlock(h) => h.SetFontFamily(ff)?,
        _ => {
            if let Ok(ctl) = handle.cast_inner::<bindings::IControl>() {
                ctl.SetFontFamily(ff)?;
            } else {
                diag::unhandled_modifier("set_prop", Prop::FontFamily, handle);
            }
        }
    }
    Ok(true)
}

fn set_str_items(
    vec: &windows_collections::IVector<windows_core::IInspectable>,
    items: &[String],
) -> Result<()> {
    vec.Clear()?;
    for s in items {
        let insp = windows_reference::IReference::from(s.as_str());
        vec.Append(&insp)?;
    }
    Ok(())
}

fn str_list_as_ivector(
    items: &[String],
) -> windows_collections::IVector<windows_core::IInspectable> {
    let vec: Vec<Option<windows_core::IInspectable>> = items
        .iter()
        .map(|s| Some(windows_reference::IReference::from(s.as_str()).into()))
        .collect();
    vec.into()
}

/// Boxed indices let drag-reorder be read back as a permutation.
fn box_index(i: usize) -> windows_core::IInspectable {
    windows_reference::IReference::<i32>::from(i as i32).into()
}

fn unbox_index(value: &windows_core::IInspectable) -> Option<usize> {
    let r = value.cast::<windows_reference::IReference<i32>>().ok()?;
    usize::try_from(r.Value().ok()?).ok()
}

const CONTENT_TEMPLATE_XAML: &str = "<DataTemplate xmlns='http://schemas.microsoft.com/winfx/2006/xaml/presentation'><ContentControl HorizontalContentAlignment='Stretch' VerticalContentAlignment='Stretch'/></DataTemplate>";

impl Backend for WinUIBackend {
    fn create(&mut self, kind: ControlKind) -> ControlId {
        let id = self.alloc_id();
        let handle = Self::make_handle_for_kind(kind);
        self.controls.borrow_mut().insert(id, handle);
        id
    }
    fn set_prop(&mut self, id: ControlId, prop: Prop, value: &PropValue) {
        // QAQ B2 手工补臂：Elevation 不走通用 prop——receiver 需父句柄，
        // prop 阶段元素尚未插入（mount_widget 先于 insert_child），暂存后延迟解析。
        if prop == Prop::Elevation {
            match value {
                PropValue::F64(z) => {
                    let live = self.elevation_live.borrow().contains(&id);
                    self.elevation.borrow_mut().insert(id, *z);
                    if live {
                        if let Some(handle) = self.controls.borrow().get(&id) {
                            diag::dropped(handle.as_ui_element().SetTranslation(
                                windows_numerics::Vector3 { x: 0.0, y: 0.0, z: *z as f32 },
                            ));
                        }
                    }
                }
                PropValue::Unset => {
                    self.elevation.borrow_mut().remove(&id);
                    self.elevation_live.borrow_mut().remove(&id);
                }
                _ => {}
            }
            return;
        }
        let map = self.controls.borrow();
        let handle = map
            .get(&id)
            .unwrap_or_else(|| panic!("WinUIBackend::set_prop: unknown control {id}"));
        let result: Result<()> = (|| -> Result<()> {
            if generated_set_prop::dispatch(handle, prop, value)? {
                return Ok(());
            }
            if let (Prop::Resources, PropValue::Resources(resources)) = (prop, value) {
                self.set_resources(id, handle, resources)?;
                return Ok(());
            }
            if try_universal_prop(handle, prop, value)? {
                // ContentDialog 尺寸需在关闭动画后恢复（Closing 会缩到 1x1），
                // 这里记录渲染层最近设置值，供重新打开前回写。
                if matches!(handle, Handle::ContentDialog(_)) {
                    match (prop, value) {
                        (Prop::Width, PropValue::F64(v)) => {
                            self.dialog_state
                                .borrow_mut()
                                .entry(id)
                                .or_default()
                                .width = Some(*v);
                        }
                        (Prop::Height, PropValue::F64(v)) => {
                            self.dialog_state
                                .borrow_mut()
                                .entry(id)
                                .or_default()
                                .height = Some(*v);
                        }
                        _ => {}
                    }
                }
                return Ok(());
            }
            match (prop, value, handle) {
                (Prop::MaxLines, PropValue::I32(v), Handle::TextBlock(text)) => {
                    text.SetMaxLines(*v)
                }
                (Prop::MaxLines, PropValue::Unset, Handle::TextBlock(text)) => text.SetMaxLines(0),
                (Prop::LineHeight, PropValue::F64(v), Handle::TextBlock(text)) => {
                    text.SetLineHeight(*v)
                }
                (Prop::LineHeight, PropValue::Unset, Handle::TextBlock(text)) => {
                    text.SetLineHeight(0.0)
                }
                (Prop::LineHeight, PropValue::F64(v), Handle::RichTextBlock(text)) => {
                    text.SetLineHeight(*v)
                }
                (Prop::LineHeight, PropValue::Unset, Handle::RichTextBlock(text)) => {
                    text.SetLineHeight(0.0)
                }
                (Prop::TextAlignment, PropValue::I32(v), Handle::TextBlock(text)) => {
                    text.SetTextAlignment(TextAlignment(*v))
                }
                (Prop::TextAlignment, PropValue::Unset, Handle::TextBlock(text)) => {
                    text.SetTextAlignment(TextAlignment::Left)
                }
                (Prop::TextTrimming, PropValue::I32(v), Handle::TextBlock(text)) => {
                    text.SetTextTrimming(TextTrimming(*v))
                }
                (Prop::TextTrimming, PropValue::Unset, Handle::TextBlock(text)) => {
                    text.SetTextTrimming(TextTrimming::None)
                }
                (Prop::IsTextSelectionEnabled, PropValue::Bool(v), Handle::RichTextBlock(tb)) => {
                    tb.SetIsTextSelectionEnabled(*v)
                }
                (Prop::IsTextSelectionEnabled, PropValue::Unset, Handle::RichTextBlock(tb)) => {
                    tb.SetIsTextSelectionEnabled(false)
                }
                (Prop::TextWrappingWrap, PropValue::I32(v), Handle::RichTextBlock(tb)) => {
                    tb.SetTextWrapping(TextWrapping(*v))
                }
                (Prop::Content, PropValue::Str(s), Handle::Button(b)) => {
                    let cc = b.cast::<bindings::IContentControl>()?;
                    // Preserve an existing icon+text layout when only text changes.
                    if let Ok(existing) = cc.Content()
                        && let Ok(panel) = existing.cast::<bindings::IPanel>()
                    {
                        let children = panel.Children()?;
                        if children.Size()? >= 2
                            && let Ok(tb) = children.GetAt(1)?.cast::<bindings::ITextBlock>()
                        {
                            return tb.SetText(s);
                        }
                    }
                    let tb = string_as_textblock(s)?;
                    cc.SetContent(&tb)
                }
                (Prop::Icon, PropValue::Icon(icon), Handle::Button(b)) => {
                    let icon_elem = build_icon_element(icon)?;
                    let cc = b.cast::<bindings::IContentControl>()?;
                    // Preserve text when replacing an existing icon.
                    if let Ok(existing) = cc.Content()
                        && let Ok(panel) = existing.cast::<bindings::IPanel>()
                    {
                        let children = panel.Children()?;
                        if children.Size()? >= 2 {
                            children.SetAt(0, &icon_elem.cast::<bindings::UIElement>()?)?;
                            return Ok(());
                        }
                    }
                    let use_icon_only = if let Ok(existing) = cc.Content() {
                        existing.cast::<bindings::IIconElement>().is_ok()
                            || existing
                                .cast::<bindings::ITextBlock>()
                                .ok()
                                .and_then(|tb| tb.Text().ok())
                                .is_some_and(|t| t.is_empty())
                    } else {
                        true
                    };
                    if use_icon_only {
                        cc.SetContent(&icon_elem)
                    } else {
                        let panel = bindings::StackPanel::new()?;
                        panel.SetOrientation(Orientation::Horizontal)?;
                        panel.SetSpacing(8.0)?;
                        let children = panel.cast::<bindings::IPanel>()?.Children()?;
                        children.Append(&icon_elem.cast::<bindings::UIElement>()?)?;
                        if let Ok(existing) = cc.Content()
                            && let Ok(ui) = existing.cast::<bindings::UIElement>()
                        {
                            children.Append(&ui)?;
                        }
                        cc.SetContent(&panel)
                    }
                }
                (Prop::Icon, PropValue::Unset, Handle::Button(b)) => {
                    let cc = b.cast::<bindings::IContentControl>()?;
                    let Ok(existing) = cc.Content() else {
                        return Ok(());
                    };
                    // Unwrap icon+text layout back to text-only.
                    if let Ok(panel) = existing.cast::<bindings::IPanel>() {
                        let children = panel.Children()?;
                        if children.Size()? >= 2 {
                            let text_child = children.GetAt(1)?;
                            children.Clear()?;
                            return cc.SetContent(&text_child);
                        }
                    }
                    if existing.cast::<bindings::IIconElement>().is_ok() {
                        return cc.SetContent(None::<&windows_core::IInspectable>);
                    }
                    Ok(())
                }
                (Prop::StyleVariant, PropValue::I32(v), Handle::Button(b)) => {
                    let fe = b.cast::<bindings::IFrameworkElement>()?;
                    let style_key = match *v {
                        1 => Some("AccentButtonStyle"),
                        2 => Some("SubtleButtonStyle"),
                        3 => Some("TextBlockButtonStyle"),
                        _ => None, // 0 = Default
                    };
                    if let Some(key_str) = style_key {
                        let resources =
                            bindings::Application::Current().and_then(|app| app.Resources())?;
                        let key = windows_reference::IReference::from(windows_core::HSTRING::from(
                            key_str,
                        ));
                        let map = resources.cast::<windows_collections::IMap<
                            windows_core::IInspectable,
                            windows_core::IInspectable,
                        >>()?;
                        if let Ok(style_obj) = map.Lookup(&key)
                            && let Ok(s) = style_obj.cast::<bindings::Style>()
                        {
                            fe.SetStyle(&s)?;
                        }
                    } else {
                        fe.SetStyle(None)?;
                    }
                    Ok(())
                }
                (Prop::Value, PropValue::Str(s), Handle::TextBox(t)) => {
                    if t.Text().ok().as_deref() == Some(s.as_str()) {
                        return Ok(());
                    }
                    t.SetText(s.as_str())
                }
                (Prop::GridRows, PropValue::GridLengths(rows), Handle::Grid(g)) => {
                    let defs = g.RowDefinitions()?;
                    defs.Clear()?;
                    for r in rows {
                        let rd = bindings::RowDefinition::new()?;
                        rd.SetHeight(to_xaml_gridlength(*r)?)?;
                        defs.Append(&rd)?;
                    }
                    Ok(())
                }
                (Prop::GridColumns, PropValue::GridLengths(cols), Handle::Grid(g)) => {
                    let defs = g.ColumnDefinitions()?;
                    defs.Clear()?;
                    for c in cols {
                        let cd = bindings::ColumnDefinition::new()?;
                        cd.SetWidth(to_xaml_gridlength(*c)?)?;
                        defs.Append(&cd)?;
                    }
                    Ok(())
                }
                (Prop::Step, PropValue::F64(v), Handle::Slider(s)) => {
                    s.SetStepFrequency(*v)?;
                    s.cast::<bindings::IRangeBase>()?.SetSmallChange(*v)
                }
                (Prop::Step, PropValue::Unset, Handle::Slider(s)) => {
                    s.SetStepFrequency(1.0)?;
                    s.cast::<bindings::IRangeBase>()?.SetSmallChange(1.0)
                }
                // QAQ B1 手工补臂：Slider 刻度三件套（composer 强度柄，2026-08-29；
                // regen 时需保留）。
                (Prop::TickFrequency, PropValue::F64(v), Handle::Slider(s)) => {
                    s.SetTickFrequency(*v)
                }
                (Prop::TickFrequency, PropValue::Unset, Handle::Slider(s)) => {
                    s.SetTickFrequency(1.0)
                }
                (Prop::SnapsTo, PropValue::I32(v), Handle::Slider(s)) => {
                    s.SetSnapsTo(SnapsTo(*v))
                }
                (Prop::SnapsTo, PropValue::Unset, Handle::Slider(s)) => {
                    s.SetSnapsTo(SnapsTo::StepValues)
                }
                (Prop::TickPlacement, PropValue::I32(v), Handle::Slider(s)) => {
                    s.SetTickPlacement(TickPlacement(*v))
                }
                (Prop::TickPlacement, PropValue::Unset, Handle::Slider(s)) => {
                    s.SetTickPlacement(TickPlacement::None)
                }
                (Prop::NavigateUri, PropValue::Str(s), Handle::HyperlinkButton(h)) => {
                    let uri = bindings::Uri::CreateUri(s.as_str())?;
                    h.SetNavigateUri(&uri)
                }
                (Prop::NavigateUri, PropValue::Unset, Handle::HyperlinkButton(h)) => {
                    h.SetNavigateUri(None)
                }
                (Prop::IsClosable, PropValue::Bool(v), Handle::TabViewItem(ti)) => {
                    ti.SetIsClosable(*v)
                }
                (Prop::IsOpen, PropValue::Bool(v), Handle::ContentDialog(d)) => {
                    if *v {
                        // 关闭动画进行中（IsOpen=false 已应用、Closed 未触发）收到
                        // 重开请求：WinUI 拒绝动画中的 ShowAsync，对话框会卡在
                        // 「遮罩常驻、Closed 不触发」的僵死态且无法自愈。跳过本次
                        // 重开让关闭正常走完；Closed 后 app 侧 on_closed 复位状态，
                        // 下一次打开请求照常（代价仅是丢一次极端的快速重开）。
                        if self
                            .dialog_state
                            .borrow()
                            .get(&id)
                            .is_some_and(|st| st.closing)
                        {
                            self.dialog_hide_pending.borrow_mut().remove(&id);
                            diag::warn(format_args!(
                                "ContentDialog {id} reopen ignored - close animation in progress"
                            ));
                            return Ok(());
                        }
                        // 取消可能排队的延迟 Hide（✕ 后快速重开）。
                        if let Some(flag) = self.dialog_hide_pending.borrow_mut().remove(&id) {
                            flag.store(false, Ordering::Relaxed);
                        }
                        // 上次关闭（Closing）把内容清空、尺寸缩到 1x1；重新
                        // 打开前先恢复，避免 1x1 空壳弹出。
                        if let Some(st) = self.dialog_state.borrow().get(&id) {
                            if let (Some(w), Some(h)) = (st.width, st.height)
                                && let Ok(fe) = d.cast::<bindings::IFrameworkElement>()
                            {
                                diag::dropped(fe.SetWidth(w));
                                diag::dropped(fe.SetHeight(h));
                            }
                            if let Some(cid) = st.content_id
                                && let Some(content_handle) = self.controls.borrow().get(&cid)
                            {
                                let ui_elem = content_handle.as_ui_element();
                                let insp = ui_elem.cast::<windows_core::IInspectable>().ok();
                                if let Ok(cc) = d.cast::<bindings::IContentControl>() {
                                    diag::dropped(cc.SetContent(insp.as_ref()));
                                }
                            }
                        }
                        // 首次打开时挂 Closing 清理：动画播放前清空内容 + 缩小，
                        // 关闭动画只剩 1x1 空壳 —— WinUI 关闭残影（面板形状阴影
                        // 拦截输入）即由此规避。闭包零捕获（只用 sender）。
                        if !self
                            .dialog_state
                            .borrow()
                            .get(&id)
                            .map(|s| s.closing_attached)
                            .unwrap_or(false)
                        {
                            if let Ok(rev) = d.Closing(move |sender, _args| {
                                let Some(sender) = sender.as_ref() else { return };
                                if let Ok(cc) = sender.cast::<bindings::IContentControl>() {
                                    diag::dropped(cc.SetContent(
                                        None::<&windows_core::IInspectable>,
                                    ));
                                }
                                if let Ok(fe) = sender.cast::<bindings::IFrameworkElement>() {
                                    diag::dropped(fe.SetWidth(1.0));
                                    diag::dropped(fe.SetHeight(1.0));
                                }
                            }) {
                                self.dialog_state
                                    .borrow_mut()
                                    .entry(id)
                                    .or_default()
                                    .closing_attached = true;
                                self.event_revokers
                                    .borrow_mut()
                                    .insert((id, Event::Closed), vec![rev]);
                            }
                        }
                        // ContentDialog is not in the tree, so borrow another XamlRoot.
                        let xroot = self
                            .controls
                            .borrow()
                            .values()
                            .filter_map(|h| match h {
                                Handle::ContentDialog(_) => None,
                                other => other
                                    .as_ui_element()
                                    .cast::<bindings::IUIElement>()
                                    .ok()
                                    .and_then(|u| u.XamlRoot().ok()),
                            })
                            .next();
                        match xroot {
                            Some(root) => {
                                diag::dropped(d.cast::<bindings::IUIElement>()?.SetXamlRoot(&root));
                                diag::dropped(d.ShowAsync());
                                // ShowAsync 发起成功（异步，不保证弹层已可见）即记
                                // shown：IsOpen=false 只有在 shown 时才进入 closing。
                                if let Some(st) = self.dialog_state.borrow_mut().get_mut(&id) {
                                    st.shown = true;
                                }
                            }
                            None => {
                                diag::warn(format_args!(
                                    "ContentDialog.is_open ignored - no XamlRoot available"
                                ));
                            }
                        }
                        Ok(())
                    } else {
                        // 仅当对话框确实处于打开状态（ShowAsync 已发起）时进入
                        // closing：对话框已关闭后（Esc 路径 on_closed 补发的
                        // IsOpen=false）不置位，避免下次打开被关闭动画防护误拦。
                        if let Some(st) = self.dialog_state.borrow_mut().get_mut(&id)
                            && st.shown
                        {
                            st.shown = false;
                            st.closing = true;
                        }
                        // 程序化关闭（✕）：不直接在 Click 事件栈内 Hide ——
                        // WinUI 在 pointer/焦点事件处理中关闭承载 Popup 会留下
                        // 视觉残留（Esc 是系统路径，不受影响）。推迟到
                        // DispatcherQueue 下一周期（事件链完成后）再 Hide。
                        let flag = Rc::new(AtomicBool::new(true));
                        self.dialog_hide_pending.borrow_mut().insert(id, flag.clone());
                        let hide_now = |d: &bindings::ContentDialog| {
                            if flag.load(Ordering::Relaxed) {
                                diag::dropped(d.Hide());
                            }
                        };
                        match DispatcherQueue::GetForCurrentThread() {
                            Ok(queue) => {
                                let d2 = d.clone();
                                let flag2 = flag.clone();
                                let handler = DispatcherQueueHandler::new(move || {
                                    if flag2.load(Ordering::Relaxed) {
                                        diag::dropped(d2.Hide());
                                    }
                                });
                                if queue
                                    .TryEnqueueWithPriority(
                                        DispatcherQueuePriority::Normal,
                                        &handler,
                                    )
                                    .unwrap_or(false)
                                {
                                    Ok(())
                                } else {
                                    hide_now(d);
                                    Ok(())
                                }
                            }
                            Err(_) => {
                                hide_now(d);
                                Ok(())
                            }
                        }
                    }
                }
                (Prop::Value, PropValue::I32(v), Handle::InfoBadge(ib)) => {
                    if *v < 0 {
                        ib.SetValue(-1)
                    } else {
                        ib.SetValue(*v)
                    }
                }
                (Prop::DisplayName, PropValue::Unset, Handle::PersonPicture(p)) => {
                    p.SetDisplayName("")
                }
                (Prop::Initials, PropValue::Unset, Handle::PersonPicture(p)) => p.SetInitials(""),
                (Prop::CornerRadius, PropValue::F64(v), Handle::Rectangle(r)) => {
                    r.SetRadiusX(*v).and_then(|_| r.SetRadiusY(*v))
                }
                (Prop::CornerRadius, PropValue::Unset, Handle::Rectangle(r)) => {
                    r.SetRadiusX(0.0).and_then(|_| r.SetRadiusY(0.0))
                }
                (Prop::CornerRadius, PropValue::F64(v), Handle::Border(b)) => {
                    b.SetCornerRadius(bindings::CornerRadius {
                        top_left: *v,
                        top_right: *v,
                        bottom_right: *v,
                        bottom_left: *v,
                    })
                }
                (Prop::CornerRadius, PropValue::Unset, Handle::Border(b)) => {
                    b.SetCornerRadius(bindings::CornerRadius::default())
                }
                (Prop::BorderBrush, PropValue::Color(br), h) => {
                    set_border_brush(h, &solid_brush(*br)?)
                }
                (Prop::BorderBrush, PropValue::Unset, h) => {
                    set_border_brush(h, None::<&bindings::Brush>)
                }
                (Prop::BorderThickness, PropValue::Thickness(t), h) => set_border_thickness(h, *t),
                (Prop::BorderThickness, PropValue::Unset, h) => {
                    set_border_thickness(h, Thickness::default())
                }
                (Prop::LineEndpoints, PropValue::LineEndpoints(p), Handle::Line(l)) => l
                    .SetX1(p.x1)
                    .and_then(|_| l.SetY1(p.y1))
                    .and_then(|_| l.SetX2(p.x2))
                    .and_then(|_| l.SetY2(p.y2)),
                (Prop::ImageSource, PropValue::ImageSource(source), Handle::Image(img)) => {
                    match build_image_source(source)? {
                        Some(source) => img.SetSource(&source),
                        None => img.SetSource(None),
                    }
                }
                (Prop::ImageSource, PropValue::Unset, Handle::Image(img)) => img.SetSource(None),
                (Prop::Header, PropValue::Str(s), Handle::TabViewItem(ti)) => {
                    let tb = string_as_textblock(s)?;
                    ti.SetHeader(&tb)
                }
                (Prop::Header, PropValue::Str(s), Handle::Expander(e)) => {
                    let tb = string_as_textblock(s)?;
                    e.SetHeader(&tb)
                }
                (Prop::Header, PropValue::Unset, Handle::Expander(e)) => e.SetHeader(None),
                (Prop::ItemKey, PropValue::Str(s), Handle::TabViewItem(ti)) => {
                    let tag = windows_reference::IReference::from(s.as_str());
                    ti.cast::<bindings::IFrameworkElement>()?.SetTag(&tag)
                }
                (Prop::ItemKey, PropValue::Unset, Handle::TabViewItem(ti)) => {
                    ti.cast::<bindings::IFrameworkElement>()?.SetTag(None)
                }
                (Prop::MenuItems, PropValue::NavMenuItems(items), Handle::NavigationView(nv)) => {
                    let menu = nv.MenuItems()?;
                    menu.Clear()?;
                    for item in items {
                        let nv_item = build_nav_view_item(item)?;
                        menu.Append(&nv_item)?;
                    }
                    Ok(())
                }
                (Prop::SelectedTag, PropValue::Str(tag), Handle::NavigationView(nv)) => {
                    select_nav_item_by_tag(nv, tag)
                }
                (Prop::SelectedTag, PropValue::Unset, Handle::NavigationView(nv)) => {
                    nv.SetSelectedItem(None)
                }
                (Prop::AutoSuggestBox, PropValue::Bool(true), Handle::NavigationView(nv)) => {
                    let asb = bindings::AutoSuggestBox::new()?;
                    nv.SetAutoSuggestBox(&asb)
                }
                (Prop::AutoSuggestBox, PropValue::Bool(false), Handle::NavigationView(nv)) => {
                    nv.SetAutoSuggestBox(None)
                }
                (Prop::AutoSuggestPlaceholder, PropValue::Str(s), Handle::NavigationView(nv)) => {
                    if let Ok(asb) = nv.AutoSuggestBox() {
                        asb.SetPlaceholderText(s.as_str())?;
                    }
                    Ok(())
                }
                (Prop::AutoSuggestItems, PropValue::StrList(items), Handle::NavigationView(nv)) => {
                    if let Ok(asb) = nv.AutoSuggestBox() {
                        asb.cast::<bindings::IItemsControl>()?
                            .SetItemsSource(&str_list_as_ivector(items))?;
                    }
                    Ok(())
                }
                (Prop::Tall, PropValue::Bool(v), Handle::TitleBar(_)) => {
                    if let Some(state) = self.window_state.borrow().as_ref() {
                        state.set_titlebar_height(*v);
                    }
                    Ok(())
                }
                (Prop::IsBackButtonVisible, PropValue::Bool(v), Handle::NavigationView(nv)) => {
                    let val = if *v {
                        bindings::NavigationViewBackButtonVisible::Auto
                    } else {
                        bindings::NavigationViewBackButtonVisible::Collapsed
                    };
                    nv.cast::<bindings::INavigationView2>()?
                        .SetIsBackButtonVisible(val)
                }
                (Prop::ItemHeader, PropValue::Str(s), Handle::PivotItem(pi)) => {
                    let tb = string_as_textblock(s)?;
                    pi.SetHeader(&tb)
                }
                (Prop::Items, PropValue::StrList(items), Handle::BreadcrumbBar(bc)) => {
                    bc.SetItemsSource(&str_list_as_ivector(items))
                }
                (Prop::Value, PropValue::Str(s), Handle::PasswordBox(p)) => {
                    if p.Password().ok().as_deref() == Some(s.as_str()) {
                        return Ok(());
                    }
                    p.SetPassword(s.as_str())
                }
                (Prop::Value, PropValue::Unset, Handle::PasswordBox(p)) => p.SetPassword(""),
                (Prop::Items, PropValue::StrList(items), Handle::RadioButtons(r)) => {
                    set_str_items(&r.Items()?.cast()?, items)
                }
                (Prop::Items, PropValue::StrList(items), Handle::ComboBox(c)) => set_str_items(
                    &c.cast::<bindings::IItemsControl>()?.Items()?.cast()?,
                    items,
                ),
                (Prop::ColorValue, PropValue::Color(c), Handle::ColorPicker(cp)) => cp.SetColor(*c),
                (Prop::Items, PropValue::StrList(items), Handle::ListBox(lb)) => set_str_items(
                    &lb.cast::<bindings::IItemsControl>()?.Items()?.cast()?,
                    items,
                ),
                (Prop::Text, PropValue::Str(s), Handle::AutoSuggestBox(asb)) => {
                    // Skip SetText when the control already has this value -
                    // calling SetText during a user-initiated TextChanged
                    // cycle steals focus from the input field.
                    if asb.Text().ok().as_deref() == Some(s.as_str()) {
                        return Ok(());
                    }
                    asb.SetText(s)
                }
                (Prop::Items, PropValue::StrList(items), Handle::AutoSuggestBox(asb)) => asb
                    .cast::<bindings::IItemsControl>()?
                    .SetItemsSource(&str_list_as_ivector(items)),
                (Prop::DisplayMode, PropValue::I32(m), Handle::SplitView(sv)) => {
                    sv.SetDisplayMode(bindings::SplitViewDisplayMode(*m))
                }
                (Prop::Items, PropValue::MenuBarItems(items), Handle::MenuBar(mb)) => {
                    let winui_items = mb.Items()?;
                    winui_items.Clear()?;
                    for bar_item_def in items {
                        let mbi = bindings::MenuBarItem::new()?;
                        mbi.SetTitle(&bar_item_def.title)?;
                        let flyout_items = mbi.Items()?;
                        for menu_def in &bar_item_def.items {
                            let fi = build_menu_flyout_item_base(menu_def)?;
                            flyout_items.Append(&fi)?;
                        }
                        winui_items.Append(&mbi)?;
                    }
                    let handlers = self.menu_click_handlers.borrow();
                    if let Some(handler) = handlers.get(&id) {
                        let revs = Self::wire_menu_bar_clicks(mb, handler);
                        if !revs.is_empty() {
                            self.event_revokers
                                .borrow_mut()
                                .insert((id, Event::ItemClicked), revs);
                        }
                    }
                    Ok(())
                }
                (
                    Prop::MenuFlyoutItems,
                    PropValue::MenuFlyoutItems(items),
                    Handle::DropDownButton(btn),
                ) => {
                    let flyout = bindings::MenuFlyout::new()?;
                    let flyout_items = flyout.Items()?;
                    for def in items {
                        let fi = build_menu_flyout_item_base(def)?;
                        flyout_items.Append(&fi)?;
                    }
                    btn.cast::<bindings::IButton>()?.SetFlyout(&flyout)?;
                    let handlers = self.menu_click_handlers.borrow();
                    if let Some(handler) = handlers.get(&id) {
                        let revs = Self::wire_flyout_clicks(&flyout, handler);
                        if !revs.is_empty() {
                            self.event_revokers
                                .borrow_mut()
                                .insert((id, Event::ItemClicked), revs);
                        }
                    }
                    Ok(())
                }
                (Prop::MenuFlyoutItems, PropValue::MenuFlyoutItems(items), Handle::Button(btn)) => {
                    let flyout = bindings::MenuFlyout::new()?;
                    let flyout_items = flyout.Items()?;
                    for def in items {
                        let fi = build_menu_flyout_item_base(def)?;
                        flyout_items.Append(&fi)?;
                    }
                    btn.SetFlyout(&flyout)?;
                    let handlers = self.menu_click_handlers.borrow();
                    if let Some(handler) = handlers.get(&id) {
                        let revs = Self::wire_flyout_clicks(&flyout, handler);
                        if !revs.is_empty() {
                            self.event_revokers
                                .borrow_mut()
                                .insert((id, Event::ItemClicked), revs);
                        }
                    }
                    Ok(())
                }
                (
                    Prop::CommandBarFlyoutCommands,
                    PropValue::CommandBarFlyoutDef { primary, secondary },
                    Handle::Button(btn),
                ) => {
                    let flyout = bindings::CommandBarFlyout::new()?;
                    let primary_cmds = flyout.PrimaryCommands()?;
                    let secondary_cmds = flyout.SecondaryCommands()?;
                    for def in primary {
                        let el = build_command_bar_element(def)?;
                        primary_cmds.Append(&el)?;
                    }
                    for def in secondary {
                        let el = build_command_bar_element(def)?;
                        secondary_cmds.Append(&el)?;
                    }
                    btn.SetFlyout(&flyout)?;
                    let handlers = self.command_bar_flyout_handlers.borrow();
                    if let Some(handler) = handlers.get(&id) {
                        let mut revs = Self::wire_command_bar_clicks(&primary_cmds, handler);
                        revs.extend(Self::wire_command_bar_clicks(&secondary_cmds, handler));
                        if !revs.is_empty() {
                            self.event_revokers
                                .borrow_mut()
                                .insert((id, Event::Click), revs);
                        }
                    }
                    Ok(())
                }
                (Prop::Nodes, PropValue::TreeViewNodes(nodes), Handle::TreeView(tv)) => {
                    let root = tv.RootNodes()?;
                    root.Clear()?;
                    for node_def in nodes {
                        let node = build_tree_view_node(node_def)?;
                        root.Append(&node)?;
                    }
                    Ok(())
                }
                (
                    Prop::PrimaryCommands,
                    PropValue::CommandBarCommands(cmds),
                    Handle::CommandBar(cb),
                ) => {
                    let primary = cb.PrimaryCommands()?;
                    primary.Clear()?;
                    for def in cmds {
                        let el = build_command_bar_element(def)?;
                        primary.Append(&el)?;
                    }
                    let handlers = self.menu_click_handlers.borrow();
                    if let Some(handler) = handlers.get(&id) {
                        let revs = Self::wire_command_bar_clicks(&primary, handler);
                        if !revs.is_empty() {
                            self.event_revokers
                                .borrow_mut()
                                .insert((id, Event::Click), revs);
                        }
                    }
                    Ok(())
                }
                (
                    Prop::SecondaryCommands,
                    PropValue::CommandBarCommands(cmds),
                    Handle::CommandBar(cb),
                ) => {
                    let secondary = cb.SecondaryCommands()?;
                    secondary.Clear()?;
                    for def in cmds {
                        let el = build_command_bar_element(def)?;
                        secondary.Append(&el)?;
                    }
                    let handlers = self.menu_click_handlers.borrow();
                    if let Some(handler) = handlers.get(&id) {
                        let revs = Self::wire_command_bar_clicks(&secondary, handler);
                        if !revs.is_empty() {
                            let mut rev_map = self.event_revokers.borrow_mut();
                            rev_map.entry((id, Event::Click)).or_default().extend(revs);
                        }
                    }
                    Ok(())
                }
                (Prop::ActionButton, PropValue::Str(s), Handle::TeachingTip(tt)) => {
                    let boxed: windows_core::IInspectable =
                        windows_reference::IReference::<windows_core::HSTRING>::from(
                            windows_core::HSTRING::from(s.as_str()),
                        )
                        .cast()?;
                    tt.SetActionButtonContent(&boxed)
                }
                (Prop::CloseButton, PropValue::Str(s), Handle::TeachingTip(tt)) => {
                    let boxed: windows_core::IInspectable =
                        windows_reference::IReference::<windows_core::HSTRING>::from(
                            windows_core::HSTRING::from(s.as_str()),
                        )
                        .cast()?;
                    tt.SetCloseButtonContent(&boxed)
                }
                (Prop::Items, PropValue::SelectorBarItems(items), Handle::SelectorBar(sb)) => {
                    let vec = sb.Items()?;
                    vec.Clear()?;
                    for def in items {
                        let item = bindings::SelectorBarItem::new()?;
                        item.SetText(&def.text)?;
                        if let Some(icon) = &def.icon {
                            let icon_elem = build_icon_element(icon)?;
                            item.SetIcon(&icon_elem)?;
                        }
                        vec.Append(&item)?;
                    }
                    Ok(())
                }
                (Prop::Text, PropValue::Str(s), Handle::RichEditBox(reb)) => {
                    let doc = reb.Document()?;
                    let mut current = windows_core::HSTRING::default();
                    doc.GetText(bindings::TextGetOptions::None, &mut current)
                        .ok();
                    if current == s.as_str() {
                        return Ok(());
                    }
                    let read_only = reb.IsReadOnly()?;
                    if read_only {
                        reb.SetIsReadOnly(false)?;
                    }
                    let set_result = doc.SetText(bindings::TextSetOptions::None, s.as_str());
                    let restore_result = if read_only {
                        reb.SetIsReadOnly(true)
                    } else {
                        Ok(())
                    };
                    set_result?;
                    restore_result
                }
                (Prop::Header, PropValue::Str(s), Handle::RichEditBox(reb)) => {
                    let tb = string_as_textblock(s)?;
                    reb.SetHeader(&tb)
                }
                (Prop::Header, PropValue::Unset, Handle::RichEditBox(reb)) => reb.SetHeader(None),
                (Prop::FlyoutContent, PropValue::Str(s), Handle::Button(b)) => {
                    let flyout = bindings::Flyout::new()?;
                    let tb = string_as_textblock(s)?;
                    flyout.SetContent(&tb)?;
                    b.SetFlyout(&flyout)?;
                    // Props are applied before the content slot mounts, so
                    // pending open/closed state may have arrived first.
                    self.consume_flyout_pending(id, b)?;
                    Ok(())
                }
                (Prop::FlyoutPlacement, PropValue::I32(v), Handle::Button(b)) => {
                    if let Ok(fb) = b.Flyout() {
                        diag::dropped(
                            fb.cast::<bindings::IFlyoutBase>()?
                                .SetPlacement(FlyoutPlacementMode(*v)),
                        );
                    }
                    Ok(())
                }
                (Prop::FlyoutOpen, PropValue::Bool(v), Handle::Button(b)) => {
                    match b.Flyout() {
                        Ok(flyout) => {
                            let fb = flyout.cast::<bindings::IFlyoutBase>()?;
                            if *v {
                                let target = b.cast::<bindings::FrameworkElement>()?;
                                fb.ShowAt(&target)?;
                            } else {
                                fb.Hide()?;
                            }
                        }
                        Err(_) => {
                            // Flyout not created yet (props run before the
                            // content slot mount); consumed on creation.
                            self.flyout_open_pending.borrow_mut().insert(id, *v);
                        }
                    }
                    Ok(())
                }
                (_, PropValue::Unset, _) => Ok(()),
                (p, v, h) => {
                    diag::unhandled_prop(id, p, v, h);
                    Ok(())
                }
            }
        })();
        if let Err(e) = result {
            diag::warn(format_args!("set_prop on {id}: {e:?}"));
        }
    }
    fn append_child(&mut self, parent: ControlId, child: ControlId) {
        let map = self.controls.borrow();
        let parent_h = map
            .get(&parent)
            .unwrap_or_else(|| panic!("WinUIBackend::append_child: unknown parent {parent}"));
        let child_h = map
            .get(&child)
            .unwrap_or_else(|| panic!("WinUIBackend::append_child: unknown child {child}"));
        let child_ui = child_h.as_ui_element();
        let cc = classify_container(parent_h).unwrap_or_else(|| {
            panic!(
                "WinUIBackend::append_child: {} ({parent}) is not a container",
                parent_h.kind_name()
            )
        });
        container_append(&cc, &child_ui);
    }
    fn remove_child(&mut self, parent: ControlId, index: usize) {
        let map = self.controls.borrow();
        let parent_h = map
            .get(&parent)
            .unwrap_or_else(|| panic!("WinUIBackend::remove_child: unknown parent {parent}"));
        let cc = classify_container(parent_h)
            .unwrap_or_else(|| panic!("WinUIBackend::remove_child: {parent} is not a container"));
        container_remove(&cc, index);
    }
    fn replace_child(&mut self, parent: ControlId, index: usize, new: ControlId) {
        let map = self.controls.borrow();
        let parent_h = map
            .get(&parent)
            .unwrap_or_else(|| panic!("WinUIBackend::replace_child: unknown parent {parent}"));
        let new_h = map
            .get(&new)
            .unwrap_or_else(|| panic!("WinUIBackend::replace_child: unknown child {new}"));
        let new_ui = new_h.as_ui_element();
        let cc = classify_container(parent_h)
            .unwrap_or_else(|| panic!("WinUIBackend::replace_child: {parent} is not a container"));
        container_set(&cc, index, &new_ui);
    }
    fn move_child(&mut self, parent: ControlId, from: usize, to: usize) {
        if from == to {
            return;
        }
        let map = self.controls.borrow();
        let parent_h = map
            .get(&parent)
            .unwrap_or_else(|| panic!("WinUIBackend::move_child: unknown parent {parent}"));
        let cc = classify_container(parent_h)
            .unwrap_or_else(|| panic!("WinUIBackend::move_child: {parent} is not a container"));
        container_move(&cc, from, to);
    }
    fn insert_child(&mut self, parent: ControlId, index: usize, child: ControlId) {
        let map = self.controls.borrow();
        let parent_h = map
            .get(&parent)
            .unwrap_or_else(|| panic!("WinUIBackend::insert_child: unknown parent {parent}"));
        let child_h = map
            .get(&child)
            .unwrap_or_else(|| panic!("WinUIBackend::insert_child: unknown child {child}"));
        let child_ui = child_h.as_ui_element();
        let cc = classify_container(parent_h)
            .unwrap_or_else(|| panic!("WinUIBackend::insert_child: {parent} is not a container"));
        container_insert(&cc, index, &child_ui);
        // QAQ B2：Elevation 在插入时落地——receiver = 直接父元素（悬浮卡的
        // 影子落在承载它的面板上；全 app 语义见 composer-streamline B2）。
        let z = self.elevation.borrow().get(&child).copied();
        if let Some(z) = z {
            let shadow = bindings::ThemeShadow::new().unwrap();
            let receivers = shadow.Receivers().unwrap();
            diag::dropped(receivers.Append(&parent_h.as_ui_element()));
            diag::dropped(child_ui.SetShadow(&shadow));
            diag::dropped(
                child_ui.SetTranslation(windows_numerics::Vector3 { x: 0.0, y: 0.0, z: z as f32 }),
            );
            self.elevation_live.borrow_mut().insert(child);
        }
    }
    fn set_templated_item_count(&mut self, id: ControlId, count: usize) {
        let map = self.controls.borrow();
        let Some(handle) = map.get(&id) else { return };
        let items_control: bindings::IItemsControl = match handle {
            Handle::ListView(lv) => lv.cast().unwrap(),
            Handle::GridView(gv) => gv.cast().unwrap(),
            _ => return,
        };

        let mut lists = self.templated.borrow_mut();
        let entry = lists.entry(id).or_insert_with(TemplatedList::new);
        let source_slot = &entry.shared.source;

        let mut slot = source_slot.borrow_mut();
        match slot.as_ref() {
            None => {
                // Install the template before WinUI realizes row containers.
                diag::dropped(items_control.SetItemTemplate(&self.content_template()));
                let values: Vec<Option<windows_core::IInspectable>> =
                    (0..count).map(|i| Some(box_index(i))).collect();
                let source: windows_collections::IObservableVector<windows_core::IInspectable> =
                    values.into();
                diag::dropped(items_control.SetItemsSource(&source));
                *slot = Some(source);
            }
            Some(source) => {
                let current = source.Size().unwrap_or(0) as usize;
                if count > current {
                    for i in current..count {
                        diag::dropped(source.Append(&box_index(i)));
                    }
                } else {
                    for _ in count..current {
                        diag::dropped(source.RemoveAtEnd());
                    }
                }
            }
        }
    }
    fn set_templated_row_content(
        &mut self,
        list_id: ControlId,
        row_idx: usize,
        content: Option<ControlId>,
    ) {
        let map = self.controls.borrow();
        let list_h = map
            .get(&list_id)
            .unwrap_or_else(|| panic!("set_templated_row_content: unknown list {list_id}"));
        let content_ui = content.and_then(|c| map.get(&c).map(Handle::as_ui_element));

        // ListView/GridView rows are filled through realized template containers.
        match list_h {
            Handle::ListView(_) | Handle::GridView(_) => {
                let container = self
                    .templated
                    .borrow()
                    .get(&list_id)
                    .and_then(|t| t.shared.containers.borrow().get(&row_idx).cloned());
                let Some(container) = container else { return };
                match content_ui {
                    Some(ui) => diag::dropped(container.SetContent(&ui)),
                    None => {
                        diag::dropped(container.SetContent(None::<&windows_core::IInspectable>));
                    }
                }
                return;
            }
            Handle::FlipView(_) => {}
            other => panic!(
                "set_templated_row_content: {} is not a templated list",
                describe_kind(other)
            ),
        }

        let items_control: bindings::IItemsControl = match list_h {
            Handle::FlipView(fv) => fv.cast().unwrap(),
            _ => unreachable!(),
        };
        let items = items_control
            .Items()
            .unwrap()
            .cast::<windows_collections::IVector<windows_core::IInspectable>>()
            .unwrap();
        let current_len = items.Size().unwrap() as usize;
        match content_ui {
            Some(ui) => {
                let insp: windows_core::IInspectable = ui.cast().unwrap();
                if row_idx < current_len {
                    items.SetAt(row_idx as u32, &insp).unwrap();
                } else {
                    while (items.Size().unwrap() as usize) < row_idx {
                        let pad: windows_core::IInspectable =
                            bindings::TextBlock::new().unwrap().cast().unwrap();
                        items.Append(&pad).unwrap();
                    }
                    items.Append(&insp).unwrap();
                }
            }
            None => {
                if row_idx < current_len {
                    items.RemoveAt(row_idx as u32).unwrap();
                }
            }
        }
    }
    fn set_templated_selected_index(&mut self, id: ControlId, index: i32) {
        let map = self.controls.borrow();
        let Some(handle) = map.get(&id) else { return };
        let selector: bindings::ISelector = match handle {
            Handle::ListView(lv) => lv.cast().unwrap(),
            Handle::GridView(gv) => gv.cast().unwrap(),
            Handle::FlipView(fv) => fv.cast().unwrap(),
            _ => return,
        };
        diag::dropped(selector.SetSelectedIndex(index));
    }

    fn set_templated_selection_mode(&mut self, id: ControlId, mode: SelectionMode) {
        let map = self.controls.borrow();
        let Some(handle) = map.get(&id) else { return };
        let lvb: bindings::IListViewBase = match handle {
            Handle::ListView(lv) => lv.cast().unwrap(),
            Handle::GridView(gv) => gv.cast().unwrap(),
            // FlipView doesn't support SelectionMode.
            _ => return,
        };
        use SelectionMode;
        let winui_mode = match mode {
            SelectionMode::None => bindings::ListViewSelectionMode::None,
            SelectionMode::Single => bindings::ListViewSelectionMode::Single,
            SelectionMode::Multiple => bindings::ListViewSelectionMode::Multiple,
            SelectionMode::Extended => bindings::ListViewSelectionMode::Extended,
        };
        diag::dropped(lvb.SetSelectionMode(winui_mode));
    }

    fn set_templated_can_drag_items(&mut self, id: ControlId, value: bool) {
        let map = self.controls.borrow();
        let Some(handle) = map.get(&id) else { return };
        let lvb: bindings::IListViewBase = match handle {
            Handle::ListView(lv) => lv.cast().unwrap(),
            Handle::GridView(gv) => gv.cast().unwrap(),
            _ => return,
        };
        diag::dropped(lvb.SetCanDragItems(value));
    }

    fn set_templated_can_reorder_items(&mut self, id: ControlId, value: bool) {
        let map = self.controls.borrow();
        let Some(handle) = map.get(&id) else { return };
        let lvb: bindings::IListViewBase = match handle {
            Handle::ListView(lv) => lv.cast().unwrap(),
            Handle::GridView(gv) => gv.cast().unwrap(),
            _ => return,
        };
        diag::dropped(lvb.SetCanReorderItems(value));
    }

    fn set_templated_allow_drop(&mut self, id: ControlId, value: bool) {
        let map = self.controls.borrow();
        let Some(handle) = map.get(&id) else { return };
        let ui: bindings::IUIElement = match handle {
            Handle::ListView(lv) => lv.cast().unwrap(),
            Handle::GridView(gv) => gv.cast().unwrap(),
            Handle::FlipView(fv) => fv.cast().unwrap(),
            _ => return,
        };
        diag::dropped(ui.SetAllowDrop(value));
    }

    fn set_header_element(&mut self, id: ControlId, header_id: Option<ControlId>) {
        let map = self.controls.borrow();
        let Some(handle) = map.get(&id) else { return };
        if let Handle::Expander(e) = handle {
            if let Some(hdr_id) = header_id {
                if let Some(hdr_handle) = map.get(&hdr_id) {
                    let ui_elem = hdr_handle.as_ui_element();
                    diag::dropped(e.SetHeader(&ui_elem));
                }
            } else {
                diag::dropped(e.SetHeader(None));
            }
        } else if let Handle::TitleBar(tb) = handle {
            if let Some(hdr_id) = header_id {
                if let Some(hdr_handle) = map.get(&hdr_id) {
                    let ui_elem = hdr_handle.as_ui_element();
                    diag::dropped(tb.SetContent(&ui_elem));
                }
            } else {
                diag::dropped(tb.SetContent(None));
            }
        } else if let Handle::TabViewItem(tab) = handle {
            if let Some(header_id) = header_id {
                if let Some(header_handle) = map.get(&header_id) {
                    let ui_element = header_handle.as_ui_element();
                    diag::dropped(tab.SetHeader(&ui_element));
                }
            } else {
                diag::dropped(tab.SetHeader(None));
            }
        }
    }

    fn set_pane_element(&mut self, id: ControlId, pane_id: Option<ControlId>) {
        let map = self.controls.borrow();
        let Some(handle) = map.get(&id) else { return };
        if let Handle::SplitView(sv) = handle {
            if let Some(pid) = pane_id {
                if let Some(pane_handle) = map.get(&pid) {
                    let ui_elem = pane_handle.as_ui_element();
                    diag::dropped(sv.SetPane(&ui_elem));
                }
            } else {
                diag::dropped(sv.SetPane(None));
            }
        } else if let Handle::TitleBar(tb) = handle {
            if let Some(pid) = pane_id {
                if let Some(pane_handle) = map.get(&pid) {
                    let ui_elem = pane_handle.as_ui_element();
                    diag::dropped(tb.SetRightHeader(&ui_elem));
                }
            } else {
                diag::dropped(tb.SetRightHeader(None));
            }
        } else if let Handle::NavigationView(nv) = handle {
            if let Some(pid) = pane_id {
                if let Some(pane_handle) = map.get(&pid) {
                    let ui_elem = pane_handle.as_ui_element();
                    diag::dropped(nv.SetPaneFooter(&ui_elem));
                }
            } else {
                diag::dropped(nv.SetPaneFooter(None));
            }
        }
    }

    fn set_flyout_content(&mut self, id: ControlId, content_id: Option<ControlId>) {
        let map = self.controls.borrow();
        let Some(handle) = map.get(&id) else { return };
        let Handle::Button(b) = handle else { return };
        if let Some(cid) = content_id {
            let Some(content_handle) = map.get(&cid) else {
                return;
            };
            let ui_elem = content_handle.as_ui_element();
            // Reuse an existing attached flyout (props like placement/open
            // may already have targeted it); create one lazily otherwise.
            let flyout: bindings::FlyoutBase = match b.Flyout() {
                Ok(f) => f,
                Err(_) => {
                    let Ok(f) = bindings::Flyout::new() else {
                        return;
                    };
                    diag::dropped(b.SetFlyout(&f));
                    match f.cast::<bindings::FlyoutBase>() {
                        Ok(fb) => fb,
                        Err(_) => return,
                    }
                }
            };
            diag::dropped(
                flyout
                    .cast::<bindings::IFlyout>()
                    .and_then(|i| i.SetContent(&ui_elem)),
            );
            self.consume_flyout_pending(id, b)
                .unwrap_or_else(|e| diag::warn(format_args!("set_flyout_content pending: {e:?}")));
        } else if let Ok(flyout) = b.Flyout() {
            diag::dropped(
                flyout
                    .cast::<bindings::IFlyout>()
                    .and_then(|i| i.SetContent(None::<&bindings::UIElement>)),
            );
        }
    }

    fn set_content_element(&mut self, id: ControlId, content_id: Option<ControlId>) {
        self.dialog_state
            .borrow_mut()
            .entry(id)
            .or_default()
            .content_id = content_id;
        let map = self.controls.borrow();
        let Some(handle) = map.get(&id) else { return };
        let Handle::ContentDialog(d) = handle else { return };
        let Some(cc) = d.cast::<bindings::IContentControl>().ok() else {
            return;
        };
        if let Some(cid) = content_id {
            let Some(content_handle) = map.get(&cid) else {
                return;
            };
            let ui_elem = content_handle.as_ui_element();
            let insp = ui_elem.cast::<windows_core::IInspectable>().ok();
            diag::dropped(cc.SetContent(insp.as_ref()));
        } else {
            diag::dropped(cc.SetContent(None::<&windows_core::IInspectable>));
        }
    }

    fn scroll_templated_to_index(&mut self, id: ControlId, index: i32) {
        if index < 0 {
            return;
        }
        let map = self.controls.borrow();
        let Some(handle) = map.get(&id) else { return };
        let lvb: Option<bindings::IListViewBase> = match handle {
            Handle::ListView(lv) => lv.cast().ok(),
            Handle::GridView(gv) => gv.cast().ok(),
            Handle::FlipView(fv) => {
                diag::dropped(
                    fv.cast::<bindings::ISelector>()
                        .unwrap()
                        .SetSelectedIndex(index),
                );
                None
            }
            _ => return,
        };
        if let Some(lvb) = lvb {
            let items_control: bindings::IItemsControl = match handle {
                Handle::ListView(lv) => lv.cast().unwrap(),
                Handle::GridView(gv) => gv.cast().unwrap(),
                _ => return,
            };
            if let Ok(items) = items_control.Items()
                && let Ok(coll) =
                    items.cast::<windows_collections::IVector<windows_core::IInspectable>>()
            {
                let len = coll.Size().unwrap_or(0);
                if (index as u32) < len
                    && let Ok(item) = coll.GetAt(index as u32)
                {
                    diag::dropped(lvb.ScrollIntoView(&item));
                }
            }
        }
    }
    fn configure_templated_scroll(
        &mut self,
        id: ControlId,
        top_threshold: f64,
        tail_threshold: f64,
        on_top_reached: Option<Callback<()>>,
        on_view_changed: Option<Callback<TemplatedViewport>>,
    ) {
        let scroll = {
            let mut lists = self.templated.borrow_mut();
            Rc::clone(
                &lists
                    .entry(id)
                    .or_insert_with(TemplatedList::new)
                    .shared
                    .scroll,
            )
        };
        {
            let mut state = scroll.borrow_mut();
            state.top_threshold = top_threshold.max(0.0);
            state.tail_threshold = tail_threshold.max(0.0);
            state.on_top_reached = on_top_reached;
            state.on_view_changed = on_view_changed;
        }
        self.ensure_templated_scroll_viewer(id);
    }

    fn prepare_templated_scroll(&mut self, id: ControlId, request: TemplatedScrollRequest) {
        self.ensure_templated_scroll_viewer(id);
        let scroll = {
            let mut lists = self.templated.borrow_mut();
            Rc::clone(
                &lists
                    .entry(id)
                    .or_insert_with(TemplatedList::new)
                    .shared
                    .scroll,
            )
        };
        let mut state = scroll.borrow_mut();
        state.pending = match request {
            TemplatedScrollRequest::ForceTail { .. } => Some(PreparedTemplatedScroll::Tail),
            TemplatedScrollRequest::FollowTail { .. } if state.following_tail => {
                Some(PreparedTemplatedScroll::Tail)
            }
            TemplatedScrollRequest::FollowTail { .. } => None,
            TemplatedScrollRequest::PreserveAnchor {
                index,
                viewport_offset,
                ..
            } => Some(PreparedTemplatedScroll::PreserveAnchor {
                index,
                viewport_offset,
                offset_before: state
                    .viewer
                    .as_ref()
                    .and_then(|viewer| viewer.VerticalOffset().ok())
                    .unwrap_or(0.0),
            }),
            TemplatedScrollRequest::RestoreOffset {
                vertical_offset,
                following_tail,
                ..
            } => Some(PreparedTemplatedScroll::RestoreOffset {
                vertical_offset,
                following_tail,
            }),
        };
    }

    fn apply_prepared_templated_scroll(&mut self, id: ControlId) -> bool {
        self.ensure_templated_scroll_viewer(id);
        let (scroll, item_containers) = {
            let mut lists = self.templated.borrow_mut();
            let entry = lists.entry(id).or_insert_with(TemplatedList::new);
            (
                Rc::clone(&entry.shared.scroll),
                Rc::clone(&entry.shared.item_containers),
            )
        };
        apply_prepared_templated_scroll_shared(&scroll, &item_containers)
    }
    fn attach_templated_selection_changed(&mut self, id: ControlId, handler: Callback<i32>) {
        let map = self.controls.borrow();
        let Some(handle) = map.get(&id) else { return };

        let selector: bindings::ISelector = match handle {
            Handle::ListView(lv) => lv.cast().unwrap(),
            Handle::GridView(gv) => gv.cast().unwrap(),
            Handle::FlipView(fv) => fv.cast().unwrap(),
            _ => return,
        };
        self.templated_selection_revokers.borrow_mut().remove(&id);
        let control = selector.clone();
        let revoker = selector
            .SelectionChanged(move |_sender, _args| {
                let idx = control.SelectedIndex().unwrap_or(-1);
                handler.invoke(idx);
            })
            .unwrap_or_else(|e| {
                panic!(
                    "WinUIBackend::attach_templated_selection_changed: \
                 Selector.SelectionChanged registration failed for control {id}: {e}"
                )
            });
        self.templated_selection_revokers
            .borrow_mut()
            .insert(id, revoker);
    }
    fn attach_templated_realization(
        &mut self,
        id: ControlId,
        realize: Rc<dyn Fn(usize)>,
        recycle: Rc<dyn Fn(usize)>,
    ) {
        let map = self.controls.borrow();
        let Some(handle) = map.get(&id) else { return };
        let lvb: bindings::IListViewBase = match handle {
            Handle::ListView(lv) => lv.cast().unwrap(),
            Handle::GridView(gv) => gv.cast().unwrap(),
            _ => return,
        };

        let mut lists = self.templated.borrow_mut();
        let entry = lists.entry(id).or_insert_with(TemplatedList::new);
        let containers = Rc::clone(&entry.shared.containers);
        let item_containers = Rc::clone(&entry.shared.item_containers);

        let revoker = lvb
            .ContainerContentChanging(move |_sender, args| {
                let Some(args) = args.as_ref() else { return };
                let Ok(item_container) = args.ItemContainer() else {
                    return;
                };
                // Populate the template root, not the item container.
                let Ok(root) = item_container.cast::<bindings::IContentControl>() else {
                    return;
                };
                let Some(cc) = root
                    .ContentTemplateRoot()
                    .ok()
                    .and_then(|r| r.cast::<bindings::IContentControl>().ok())
                else {
                    return;
                };
                let recycling = args.InRecycleQueue().unwrap_or(false);
                if recycling {
                    // Clear before the reconciler unmounts the row.
                    diag::dropped(cc.SetContent(None::<&windows_core::IInspectable>));
                    let mut map = containers.borrow_mut();
                    if let Some(row) = map.iter().find(|(_, c)| **c == cc).map(|(row, _)| *row) {
                        map.remove(&row);
                        item_containers.borrow_mut().remove(&row);
                        drop(map);
                        recycle(row);
                    }
                } else {
                    // Record the content host and suppress WinUI's phased rendering.
                    let row = args.ItemIndex().unwrap_or(-1);
                    if row < 0 {
                        return;
                    }
                    let row = row as usize;
                    diag::dropped(args.SetHandled(true));
                    containers.borrow_mut().insert(row, cc);
                    if let Ok(container) = item_container.cast::<bindings::IUIElement>() {
                        item_containers.borrow_mut().insert(row, container);
                    }
                    realize(row);
                }
            })
            .unwrap_or_else(|e| {
                panic!(
                    "WinUIBackend::attach_templated_realization: \
                     ContainerContentChanging registration failed for control {id}: {e}"
                )
            });
        entry.realize_revoker = Some(revoker);
    }
    fn attach_templated_reorder(&mut self, id: ControlId, handler: Callback<Vec<usize>>) {
        let map = self.controls.borrow();
        let Some(handle) = map.get(&id) else { return };
        let lvb: bindings::IListViewBase = match handle {
            Handle::ListView(lv) => lv.cast().unwrap(),
            Handle::GridView(gv) => gv.cast().unwrap(),
            _ => return,
        };

        let mut lists = self.templated.borrow_mut();
        let entry = lists.entry(id).or_insert_with(TemplatedList::new);
        let source = Rc::clone(&entry.shared.source);

        let revoker = lvb
            .DragItemsCompleted(move |_sender, _args| {
                // Read the permutation, then reset the source to identity.
                let slot = source.borrow();
                let Some(source) = slot.as_ref() else { return };
                let len = source.Size().unwrap_or(0) as usize;
                let mut order = Vec::with_capacity(len);
                for i in 0..len as u32 {
                    match source.GetAt(i).ok().as_ref().and_then(unbox_index) {
                        Some(idx) => order.push(idx),
                        None => return,
                    }
                }
                let changed = order.iter().enumerate().any(|(i, v)| *v != i);
                if !changed {
                    return;
                }
                for i in 0..len {
                    diag::dropped(source.SetAt(i as u32, &box_index(i)));
                }
                drop(slot);
                handler.invoke(order);
            })
            .unwrap_or_else(|e| {
                panic!(
                    "WinUIBackend::attach_templated_reorder: \
                     DragItemsCompleted registration failed for control {id}: {e}"
                )
            });
        entry.reorder_revoker = Some(revoker);
    }
    fn destroy(&mut self, id: ControlId) {
        self.templated_selection_revokers.borrow_mut().remove(&id);
        self.templated.borrow_mut().remove(&id);
        let captured = self
            .pointer_revokers
            .borrow_mut()
            .remove(&id)
            .is_some_and(|tokens| tokens.capture_on_press);
        if captured && let Some(handle) = self.controls.borrow().get(&id) {
            diag::dropped(handle.as_ui_element().ReleasePointerCaptures());
        }
        self.drag_revokers.borrow_mut().remove(&id);
        self.dialog_state.borrow_mut().remove(&id);
        self.dialog_hide_pending.borrow_mut().remove(&id);
        self.elevation.borrow_mut().remove(&id);
        self.elevation_live.borrow_mut().remove(&id);
        self.controls.borrow_mut().remove(&id);
        self.event_revokers
            .borrow_mut()
            .retain(|(hid, _), _| *hid != id);
        self.property_observers
            .borrow_mut()
            .retain(|(hid, _), _| *hid != id);
        self.menu_click_handlers.borrow_mut().remove(&id);
        self.command_bar_flyout_handlers.borrow_mut().remove(&id);
        self.theme_brush_registry.borrow_mut().remove(&id);
        self.resource_keys.borrow_mut().remove(&id);
        self.rich_text.borrow_mut().remove(&id);
        self.flyout_open_pending.borrow_mut().remove(&id);
        self.flyout_closed_pending.borrow_mut().remove(&id);
    }
    fn attach_event(&mut self, id: ControlId, event: Event, handler: EventHandler) {
        let map = self.controls.borrow();
        let handle = map
            .get(&id)
            .unwrap_or_else(|| panic!("WinUIBackend::attach_event: unknown control {id}"));

        if matches!(
            event,
            Event::NavigationPaneOpenChanged | Event::NavigationDisplayModeChanged
        ) && let Handle::NavigationView(navigation) = handle
        {
            self.observe_navigation_state(id, event, navigation, handler)
                .unwrap_or_else(|error| {
                    panic!(
                        "WinUIBackend::attach_event: failed to observe {event:?} \
                         for control {id}: {error}"
                    )
                });
            return;
        }

        if let Some(revs) = generated_attach_event::dispatch(handle, event, &handler) {
            if !revs.is_empty() {
                self.event_revokers.borrow_mut().insert((id, event), revs);
            }
            return;
        }

        let mut revokers: Vec<EventRevoker> = Vec::new();
        match (event, handle) {
            (Event::FlyoutClosed, Handle::Button(b)) => {
                // The flyout may not exist yet (events attach before the
                // content slot mounts); defer to `consume_flyout_pending`.
                if let Ok(flyout) = b.Flyout() {
                    if let Ok(fb) = flyout.cast::<bindings::IFlyoutBase>() {
                        let handler = handler.clone();
                        if let Ok(rev) = fb.Closed(move |_, _| handler.invoke()) {
                            revokers.push(rev);
                        }
                    }
                } else {
                    self.flyout_closed_pending.borrow_mut().insert(id, handler);
                }
            }
            (Event::Closed, Handle::ContentDialog(d)) => {
                // 复位 shown/closing：对话框已真正关闭，后续 IsOpen=true 不再被
                // 「关闭动画进行中」防护拦截（dialogs 常驻复用，状态必须回收）。
                let dialog_state = self.dialog_state.clone();
                revokers.push(
                    d.Closed(move |_sender, args| {
                        if let Some(st) = dialog_state.borrow_mut().get_mut(&id) {
                            st.shown = false;
                            st.closing = false;
                        }
                        let result = args
                            .as_ref()
                            .and_then(|a| a.Result().ok())
                            .unwrap_or(bindings::ContentDialogResult(0));
                        handler.invoke_i32(result.0);
                    })
                    .unwrap(),
                );
            }
            (Event::SelectionChanged, Handle::TabView(tv)) => {
                let control = tv.clone();
                revokers.push(
                    tv.SelectionChanged(move |_sender, _args| {
                        let idx = control.SelectedIndex().unwrap_or(-1);
                        if idx >= 0 {
                            handler.invoke_i32(idx);
                        }
                    })
                    .unwrap(),
                );
            }
            (Event::CloseRequested, Handle::TabView(tv)) => {
                revokers.push(
                    tv.TabCloseRequested(move |_sender, args| {
                        let key = args
                            .as_ref()
                            .and_then(|a| a.Tab().ok())
                            .and_then(|tab| {
                                tab.cast::<bindings::IFrameworkElement>()
                                    .unwrap()
                                    .Tag()
                                    .ok()
                            })
                            .and_then(|tag_obj| {
                                tag_obj
                                    .cast::<windows_reference::IReference<windows_core::HSTRING>>()
                                    .ok()
                                    .and_then(|pv| pv.Value().ok())
                            })
                            .map(|h| h.to_string_lossy())
                            .unwrap_or_default();
                        handler.invoke_string(key);
                    })
                    .unwrap(),
                );
            }
            (Event::SelectionChanged, Handle::NavigationView(nv)) => {
                revokers.push(
                    nv.SelectionChanged(move |_sender, args| {
                        let tag = args
                            .as_ref()
                            .and_then(|a| a.SelectedItem().ok())
                            .and_then(|item| item.cast::<bindings::NavigationViewItem>().ok())
                            .and_then(|nvi| {
                                nvi.cast::<bindings::IFrameworkElement>()
                                    .unwrap()
                                    .Tag()
                                    .ok()
                            })
                            .and_then(|tag_obj| {
                                tag_obj
                                    .cast::<windows_reference::IReference<windows_core::HSTRING>>()
                                    .ok()
                                    .and_then(|pv| pv.Value().ok())
                            })
                            .map(|h| h.to_string_lossy())
                            .unwrap_or_default();
                        handler.invoke_string(tag);
                    })
                    .unwrap(),
                );
            }
            (Event::QuerySubmitted, Handle::NavigationView(nv)) => {
                if let Ok(asb) = nv.AutoSuggestBox() {
                    revokers.push(
                        asb.QuerySubmitted(move |_sender, args| {
                            let query = args
                                .as_ref()
                                .and_then(|a| a.QueryText().ok())
                                .unwrap_or_default();
                            handler.invoke_string(query);
                        })
                        .unwrap(),
                    );
                }
            }
            (Event::TextChanged, Handle::NavigationView(nv)) => {
                if let Ok(asb) = nv.AutoSuggestBox() {
                    revokers.push(
                        asb.TextChanged(move |sender, _args| {
                            let text = sender
                                .as_ref()
                                .and_then(|s| s.Text().ok())
                                .unwrap_or_default();
                            handler.invoke_string(text);
                        })
                        .unwrap(),
                    );
                }
            }
            (Event::SuggestionChosen, Handle::NavigationView(nv)) => {
                if let Ok(asb) = nv.AutoSuggestBox() {
                    revokers.push(
                        asb.SuggestionChosen(move |_sender, args| {
                            let item = args
                                .as_ref()
                                .and_then(|a| a.SelectedItem().ok())
                                .and_then(|insp| {
                                    insp.cast::<windows_reference::IReference<
                                        windows_core::HSTRING,
                                    >>()
                                    .ok()
                                    .and_then(|pv| pv.Value().ok())
                                })
                                .map(|h| h.to_string_lossy())
                                .unwrap_or_default();
                            handler.invoke_string(item);
                        })
                        .unwrap(),
                    );
                }
            }
            (Event::SelectionChanged, Handle::Pivot(p)) => {
                let control = p.clone();
                revokers.push(
                    p.SelectionChanged(move |_sender, _args| {
                        let idx = control.SelectedIndex().unwrap_or(-1);
                        if idx >= 0 {
                            handler.invoke_i32(idx);
                        }
                    })
                    .unwrap(),
                );
            }
            (Event::SelectionChanged, Handle::ComboBox(c)) => {
                let selector: bindings::ISelector = c.cast().unwrap();
                let control = selector.clone();
                revokers.push(
                    selector
                        .SelectionChanged(move |_sender, _args| {
                            let idx = control.SelectedIndex().unwrap_or(-1);
                            handler.invoke_i32(idx);
                        })
                        .unwrap(),
                );
            }
            (Event::ColorChanged, Handle::ColorPicker(cp)) => {
                revokers.push(
                    cp.ColorChanged(move |_sender, args| {
                        let color =
                            args.as_ref()
                                .and_then(|a| a.NewColor().ok())
                                .unwrap_or(Color {
                                    a: 255,
                                    r: 0,
                                    g: 0,
                                    b: 0,
                                });
                        handler.invoke_color((color.a, color.r, color.g, color.b));
                    })
                    .unwrap(),
                );
            }
            (Event::SelectedDateChanged, Handle::DatePicker(dp)) => {
                revokers.push(
                    dp.SelectedDateChanged(move |_sender, args| {
                        if let Some(a) = args.as_ref()
                            && let Ok(dt) = a.NewDate()
                        {
                            handler.invoke_datetime(dt);
                        }
                    })
                    .unwrap(),
                );
            }
            (Event::SelectedTimeChanged, Handle::TimePicker(tp)) => {
                revokers.push(
                    tp.SelectedTimeChanged(move |_sender, args| {
                        if let Some(a) = args.as_ref()
                            && let Ok(ts) = a.NewTime()
                        {
                            handler.invoke_timespan(TimeSpan::from_ticks(ts.duration));
                        }
                    })
                    .unwrap(),
                );
            }
            (Event::DateChanged, Handle::CalendarDatePicker(cdp)) => {
                revokers.push(
                    cdp.DateChanged(move |_sender, args| {
                        if let Some(a) = args.as_ref()
                            && let Ok(dt) = a.NewDate()
                        {
                            handler.invoke_datetime(dt);
                        }
                    })
                    .unwrap(),
                );
            }
            (Event::SelectionChanged, Handle::ListBox(lb)) => {
                let selector: bindings::ISelector = lb.cast().unwrap();
                let control = selector.clone();
                revokers.push(
                    selector
                        .SelectionChanged(move |_sender, _args| {
                            if let Ok(idx) = control.SelectedIndex() {
                                handler.invoke_i32(idx);
                            }
                        })
                        .unwrap(),
                );
            }
            (Event::TextChanged, Handle::AutoSuggestBox(asb)) => {
                revokers.push(
                    asb.TextChanged(move |sender, args| {
                        // Only fire for user input, not programmatic changes.
                        let is_user_input = args
                            .as_ref()
                            .and_then(|a| a.Reason().ok())
                            .is_some_and(|r| {
                                r == bindings::AutoSuggestionBoxTextChangeReason::UserInput
                            });
                        if is_user_input {
                            let text = sender
                                .as_ref()
                                .and_then(|s| s.Text().ok())
                                .unwrap_or_default();
                            handler.invoke_string(text);
                        }
                    })
                    .unwrap(),
                );
            }
            (Event::QuerySubmitted, Handle::AutoSuggestBox(asb)) => {
                revokers.push(
                    asb.QuerySubmitted(move |_sender, args| {
                        let text = args
                            .as_ref()
                            .and_then(|a| a.QueryText().ok())
                            .unwrap_or_default();
                        handler.invoke_string(text);
                    })
                    .unwrap(),
                );
            }
            (Event::SuggestionChosen, Handle::AutoSuggestBox(asb)) => {
                revokers.push(
                    asb.SuggestionChosen(move |_sender, args| {
                        let item = args
                            .as_ref()
                            .and_then(|a| a.SelectedItem().ok())
                            .and_then(|insp| {
                                insp.cast::<windows_reference::IReference<windows_core::HSTRING>>()
                                    .ok()
                                    .and_then(|pv| pv.Value().ok())
                            })
                            .map(|h| h.to_string_lossy())
                            .unwrap_or_default();
                        handler.invoke_string(item);
                    })
                    .unwrap(),
                );
            }
            (Event::ItemClicked, Handle::MenuBar(mb)) => {
                self.menu_click_handlers
                    .borrow_mut()
                    .insert(id, handler.clone());
                let revs = Self::wire_menu_bar_clicks(mb, &handler);
                revokers.extend(revs);
            }
            (Event::ItemClicked, Handle::DropDownButton(_) | Handle::Button(_)) => {
                self.menu_click_handlers.borrow_mut().insert(id, handler);
            }
            (Event::CommandBarFlyoutClick, Handle::Button(_)) => {
                self.command_bar_flyout_handlers
                    .borrow_mut()
                    .insert(id, handler);
            }
            (Event::ItemInvoked, Handle::TreeView(tv)) => {
                revokers.push(
                    tv.ItemInvoked(move |_sender, args| {
                        let text = args
                            .as_ref()
                            .and_then(|a| a.InvokedItem().ok())
                            .and_then(|insp| {
                                insp.cast::<bindings::ITreeViewNode>()
                                    .ok()
                                    .and_then(|node| node.Content().ok())
                            })
                            .and_then(|content| {
                                content
                                    .cast::<windows_reference::IReference<windows_core::HSTRING>>()
                                    .ok()
                                    .and_then(|r| r.Value().ok())
                            })
                            .map(|h| h.to_string_lossy())
                            .unwrap_or_default();
                        handler.invoke_string(text);
                    })
                    .unwrap(),
                );
            }
            (Event::Click, Handle::CommandBar(cb)) => {
                self.menu_click_handlers
                    .borrow_mut()
                    .insert(id, handler.clone());
                if let Ok(primary) = cb.PrimaryCommands() {
                    let revs = Self::wire_command_bar_clicks(&primary, &handler);
                    revokers.extend(revs);
                }
                if let Ok(secondary) = cb.SecondaryCommands() {
                    let revs = Self::wire_command_bar_clicks(&secondary, &handler);
                    revokers.extend(revs);
                }
            }
            (Event::SelectionChanged, Handle::SelectorBar(sb)) => {
                let sb2 = sb.clone();
                revokers.push(
                    sb.SelectionChanged(move |_sender, _args| {
                        if let Ok(selected) = sb2.SelectedItem()
                            && let Ok(text) = selected.Text()
                        {
                            handler.invoke_string(text);
                        }
                    })
                    .unwrap(),
                );
            }
            (Event::TextChanged, Handle::RichEditBox(reb)) => {
                let control = reb.clone();
                revokers.push(
                    reb.TextChanged(move |_sender, _args| {
                        let text = control
                            .Document()
                            .ok()
                            .and_then(|doc| {
                                let mut buf = windows_core::HSTRING::default();
                                doc.GetText(bindings::TextGetOptions::None, &mut buf).ok()?;
                                Some(buf.to_string_lossy())
                            })
                            .unwrap_or_default();
                        handler.invoke_string(text);
                    })
                    .unwrap(),
                );
            }
            (Event::Closed, _) => {}
            (event, _) => {
                panic!("WinUIBackend::attach_event: {event:?} on unexpected control {id}")
            }
        }
        drop(map);
        if !revokers.is_empty() {
            self.event_revokers
                .borrow_mut()
                .insert((id, event), revokers);
        }
    }
    fn detach_event(&mut self, id: ControlId, event: Event) {
        self.event_revokers.borrow_mut().remove(&(id, event));
        self.property_observers.borrow_mut().remove(&(id, event));
    }
    fn set_theme_bindings(
        &mut self,
        id: ControlId,
        kind: ControlKind,
        bindings: &[(Prop, ThemeRef)],
    ) {
        let _ = kind;
        if bindings.is_empty() {
            self.theme_brush_registry.borrow_mut().remove(&id);
            let map = self.controls.borrow();
            if let Some(handle) = map.get(&id)
                && let Some((_, fe)) = style_target_for_handle(handle)
            {
                diag::dropped(fe.SetStyle(None));
            }
            return;
        }
        self.theme_brush_registry
            .borrow_mut()
            .insert(id, bindings.to_vec());

        let map = self.controls.borrow();
        let Some(handle) = map.get(&id) else {
            return;
        };
        apply_theme_resource_style(handle, bindings);
    }
    fn on_theme_changed(&mut self) {
        // Re-apply so WinUI re-resolves {ThemeResource}.
        let controls = self.controls.borrow();
        let registry = self.theme_brush_registry.borrow();
        for (id, bindings) in registry.iter() {
            let Some(handle) = controls.get(id) else {
                continue;
            };
            apply_theme_resource_style(handle, bindings);
        }
    }
    fn set_accessibility(&mut self, id: ControlId, accessibility: &AccessibilityModifiers) {
        let map = self.controls.borrow();
        let Some(handle) = map.get(&id) else {
            return;
        };
        let fe = handle.as_framework_element();
        let dep: bindings::DependencyObject = match fe.cast() {
            Ok(d) => d,
            Err(_) => return,
        };
        diag::dropped(bindings::AutomationProperties::SetName(
            &dep,
            accessibility.automation_name.as_deref().unwrap_or(""),
        ));
        diag::dropped(bindings::AutomationProperties::SetAutomationId(
            &dep,
            accessibility.automation_id.as_deref().unwrap_or(""),
        ));
        diag::dropped(bindings::AutomationProperties::SetHelpText(
            &dep,
            accessibility.help_text.as_deref().unwrap_or(""),
        ));
        let live = accessibility
            .live_setting
            .unwrap_or(AutomationLiveSetting::Off);
        diag::dropped(bindings::AutomationProperties::SetLiveSetting(&dep, live));
        let heading = accessibility
            .heading_level
            .unwrap_or(AutomationHeadingLevel::None);
        diag::dropped(bindings::AutomationProperties::SetHeadingLevel(
            &dep, heading,
        ));
    }
    fn set_keyboard_accelerators(&mut self, id: ControlId, accelerators: &[KeyboardAccelerator]) {
        let map = self.controls.borrow();
        let Some(handle) = map.get(&id) else {
            return;
        };
        let fe = handle.as_framework_element();
        let iue: bindings::IUIElement = match fe.cast() {
            Ok(i) => i,
            Err(_) => return,
        };
        let vec: windows_collections::IVector<bindings::KeyboardAccelerator> =
            match iue.KeyboardAccelerators() {
                Ok(v) => v,
                Err(_) => return,
            };
        diag::dropped(vec.Clear());

        diag::dropped(iue.SetKeyboardAcceleratorPlacementMode(
            bindings::KeyboardAcceleratorPlacementMode::Hidden,
        ));

        for accel in accelerators {
            let Ok(ka) = bindings::KeyboardAccelerator::new() else {
                continue;
            };
            let Ok(ika) = ka.cast::<bindings::IKeyboardAccelerator>() else {
                continue;
            };
            diag::dropped(ika.SetKey(accel.key));
            diag::dropped(ika.SetModifiers(accel.modifiers));
            let cb = accel.on_invoked.clone();
            let _ = ika
                .Invoked(move |_sender, args| {
                    if let Some(a) = args.as_ref() {
                        diag::dropped(a.SetHandled(true));
                    }
                    cb.invoke(());
                })
                .ok()
                .map(|r| r.into_token());
            diag::dropped(vec.Append(&ka));
        }
    }
    fn set_implicit_transitions(
        &mut self,
        id: ControlId,
        transitions: Option<ImplicitTransitions>,
    ) {
        let map = self.controls.borrow();
        let Some(handle) = map.get(&id) else {
            return;
        };
        let ui: bindings::UIElement = handle.as_ui_element();
        if let Err(e) = apply_implicit_transitions(&ui, transitions) {
            diag::warn(format_args!("set_implicit_transitions failed: {e:?}"));
        }
    }
    fn set_layout_animation(&mut self, id: ControlId, config: Option<LayoutAnimationConfig>) {
        let map = self.controls.borrow();
        let Some(handle) = map.get(&id) else {
            return;
        };
        let ui: bindings::UIElement = handle.as_ui_element();
        if let Err(e) = apply_layout_animation(&ui, config) {
            diag::warn(format_args!("set_layout_animation failed: {e:?}"));
        }
    }
    fn run_property_animation(&mut self, id: ControlId, config: Option<AnimationConfig>) {
        let Some(cfg) = config else {
            return;
        };
        let map = self.controls.borrow();
        let Some(handle) = map.get(&id) else {
            return;
        };
        let ui: bindings::UIElement = handle.as_ui_element();
        if let Err(e) = run_property_animation_inner(&ui, cfg) {
            diag::warn(format_args!("run_property_animation failed: {e:?}"));
        }
    }
    fn set_element_transitions(
        &mut self,
        id: ControlId,
        enter: Option<AnimationConfig>,
        exit: Option<AnimationConfig>,
    ) {
        let map = self.controls.borrow();
        let Some(handle) = map.get(&id) else {
            return;
        };
        let ui = handle.as_ui_element();
        if let Err(error) = apply_element_transitions(&ui, enter, exit) {
            diag::warn(format_args!("set_element_transitions failed: {error:?}"));
        }
    }
    fn set_rich_text_paragraphs(&mut self, id: ControlId, paragraphs: &[RichTextParagraph]) {
        #[cfg(debug_assertions)]
        let t0 = std::time::Instant::now();
        // 段落级 + 段内 run 级 diff：内容未变零操作（每帧 render 都走到
        // 这里）；未变化段落/run 的对象与 Blocks 位置全部保留，只重建
        // 真正变化的部分——流式期间每帧只动最后一个生长 run。
        let map = self.controls.borrow();
        let Some(handle) = map.get(&id) else {
            return;
        };
        let Handle::RichTextBlock(rtb) = handle else {
            return;
        };
        let Ok(blocks) = rtb.Blocks() else { return };
        drop(map);

        let mut cache = self.rich_text.borrow_mut();
        let state = cache.entry(id).or_insert_with(RichTextBlockState::default);
        if state.paragraphs == paragraphs {
            return;
        }
        let old = std::mem::replace(&mut state.paragraphs, paragraphs.to_vec());

        #[cfg(debug_assertions)]
        let (mut rebuilt_paras, mut new_runs, mut reused_runs) = (0usize, 0usize, 0usize);

        // 1) 不变前缀段落：对象与 Blocks 位置全部保留（零 COM 调用）。
        let common = old
            .iter()
            .zip(paragraphs.iter())
            .take_while(|(a, b)| a == b)
            .count();

        // 2) common 之后逐段对齐：段内 run 级 diff（复用段落对象与
        //    前缀 run），新增段 Append，多余段 RemoveAtEnd。
        for i in common..paragraphs.len() {
            let para_def = &paragraphs[i];
            if i < old.len() && old[i] == *para_def {
                // 与旧内容相同（common 之后个别段相等）→ 对象保留。
                continue;
            }
            #[cfg(debug_assertions)]
            {
                rebuilt_paras += 1;
            }
            let old_defs = old.get(i).map(|p| p.inlines.as_slice()).unwrap_or(&[]);
            let para = if i < state.blocks.len() {
                // 复用段落对象（Blocks 位置不动），段内 run 级 diff。
                let para = state.blocks[i].clone();
                if let Ok(inlines) = para.Inlines() {
                    let mut runs = state.runs.get_mut(i).cloned().unwrap_or_default();
                    let (n, r) =
                        sync_paragraph_inlines(&inlines, old_defs, &para_def.inlines, &mut runs);
                    state.runs[i] = runs;
                    #[cfg(debug_assertions)]
                    {
                        new_runs += n;
                        reused_runs += r;
                    }
                    #[cfg(not(debug_assertions))]
                    let _ = (n, r);
                }
                para
            } else {
                let Ok(para) = bindings::Paragraph::new() else {
                    continue;
                };
                if let Ok(inlines) = para.Inlines() {
                    let mut runs = Vec::new();
                    let (n, _) = sync_paragraph_inlines(&inlines, &[], &para_def.inlines, &mut runs);
                    state.runs.push(runs);
                    #[cfg(debug_assertions)]
                    {
                        new_runs += n;
                    }
                    #[cfg(not(debug_assertions))]
                    let _ = n;
                }
                diag::dropped(
                    para.cast::<bindings::Block>()
                        .and_then(|b| blocks.Append(&b)),
                );
                para
            };
            if i < state.blocks.len() {
                state.blocks[i] = para;
            } else {
                state.blocks.push(para);
            }
        }

        // 3) 多余段落：Blocks 尾部弹出（对象与 runs 一并丢弃）。
        while state.blocks.len() > paragraphs.len() {
            diag::dropped(blocks.RemoveAtEnd());
            state.blocks.pop();
            state.runs.pop();
        }

        #[cfg(debug_assertions)]
        {
            // 聚合输出（每 60 次调用一行），避免逐帧 eprintln 重定向
            // 到文件的 I/O 污染 reconcile 计时（实测 ~2.2ms/次）。
            use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
            static AGG_FRAMES: AtomicU32 = AtomicU32::new(0);
            static AGG_NEW_RUNS: AtomicU32 = AtomicU32::new(0);
            static AGG_REUSE_RUNS: AtomicU32 = AtomicU32::new(0);
            static AGG_REBUILT: AtomicU32 = AtomicU32::new(0);
            static AGG_US: AtomicU64 = AtomicU64::new(0);
            let f = AGG_FRAMES.fetch_add(1, Ordering::Relaxed) + 1;
            AGG_NEW_RUNS.fetch_add(new_runs as u32, Ordering::Relaxed);
            AGG_REUSE_RUNS.fetch_add(reused_runs as u32, Ordering::Relaxed);
            AGG_REBUILT.fetch_add(rebuilt_paras as u32, Ordering::Relaxed);
            AGG_US.fetch_add((t0.elapsed().as_secs_f64() * 1e6) as u64, Ordering::Relaxed);
            if f % 60 == 0 {
                eprintln!(
                    "rtb-diff-agg calls={f} avg={:.3}ms rebuilt={} runs:new={} reuse={}",
                    AGG_US.load(Ordering::Relaxed) as f64 / f as f64 / 1000.0,
                    AGG_REBUILT.load(Ordering::Relaxed),
                    AGG_NEW_RUNS.load(Ordering::Relaxed),
                    AGG_REUSE_RUNS.load(Ordering::Relaxed),
                );
            }
        }
    }

    fn set_tooltip(&mut self, id: ControlId, tooltip: Option<&Tooltip>) {
        let map = self.controls.borrow();
        let Some(handle) = map.get(&id) else {
            return;
        };
        let fe = handle.as_framework_element();
        let dep: bindings::DependencyObject = match fe.cast() {
            Ok(d) => d,
            Err(_) => return,
        };

        let inspectable: Option<windows_core::IInspectable> = match tooltip {
            None => None,
            Some(t) => match &t.content {
                TooltipContent::Text(s) => {
                    let reference = windows_reference::IReference::from(s.as_str());
                    Some(reference.into())
                }
                TooltipContent::Rich(elem) => {
                    let tt = match bindings::ToolTip::new() {
                        Ok(t) => t,
                        Err(e) => {
                            diag::warn(format_args!("ToolTip::new failed: {e:?}"));
                            return;
                        }
                    };
                    if let Some(ui) = mount_static_tooltip_element(elem)
                        && let Ok(cc) = tt.cast::<bindings::IContentControl>()
                    {
                        diag::dropped(cc.SetContent(&ui));
                    }
                    Some(tt.into())
                }
            },
        };
        diag::dropped(bindings::ToolTipService::SetToolTip(
            &dep,
            inspectable.as_ref(),
        ));

        let placement = tooltip
            .and_then(|t| t.placement)
            .map_or(bindings::PlacementMode::Top, map_placement);
        diag::dropped(bindings::ToolTipService::SetPlacement(&dep, placement));
    }

    fn set_pointer_handlers(&mut self, id: ControlId, handlers: Option<&PointerHandlers>) {
        // Remove the old token set from backend ownership before replacing it.
        let prev = self.pointer_revokers.borrow_mut().remove(&id);
        let map = self.controls.borrow();
        let Some(handle) = map.get(&id) else {
            return;
        };
        let ui = handle.as_ui_element();
        let previous_capture = prev.as_ref().is_some_and(|tokens| tokens.capture_on_press);
        let next_capture = handlers.is_some_and(|handlers| handlers.capture_pointer_on_press);
        if previous_capture && !next_capture {
            // Keep the previous capture-lost callback attached while ending
            // an active gesture.
            diag::dropped(ui.ReleasePointerCaptures());
        }
        drop(prev);

        let Some(handlers) = handlers else {
            return;
        };
        let mut tokens = PointerRevokerSet {
            capture_on_press: handlers.capture_pointer_on_press,
            ..PointerRevokerSet::default()
        };

        if let Some(cb) = handlers.on_tapped.clone() {
            tokens.tapped = ui
                .Tapped(move |_sender, _args| {
                    cb.invoke(());
                })
                .ok();
        }

        if let Some(cb) = handlers.on_right_tapped.clone() {
            tokens.right_tapped = ui
                .RightTapped(move |_sender, _args| {
                    cb.invoke(());
                })
                .ok();
        }

        if handlers.on_pointer_pressed.is_some() || handlers.capture_pointer_on_press {
            let element = ui.clone();
            let cb = handlers.on_pointer_pressed.clone();
            let capture = handlers.capture_pointer_on_press;
            tokens.pressed = ui
                .PointerPressed(move |_sender, args| {
                    let capture_succeeded = if capture {
                        args.as_ref()
                            .and_then(|args| args.Pointer().ok())
                            .is_some_and(|pointer| match element.CapturePointer(&pointer) {
                                Ok(true) => true,
                                Ok(false) => {
                                    diag::warn(format_args!("pointer capture was refused"));
                                    false
                                }
                                Err(error) => {
                                    diag::warn(format_args!("pointer capture failed: {error:?}"));
                                    false
                                }
                            })
                    } else {
                        false
                    };
                    if let Some(cb) = &cb {
                        let mut info = pointer_event_info(&element, args);
                        info.capture_succeeded = capture_succeeded;
                        cb.invoke(info);
                    }
                })
                .ok();
        }

        if handlers.on_pointer_released.is_some() || handlers.capture_pointer_on_press {
            let element = ui.clone();
            let cb = handlers.on_pointer_released.clone();
            let capture = handlers.capture_pointer_on_press;
            tokens.released = ui
                .PointerReleased(move |_sender, args| {
                    let pointer = capture
                        .then(|| args.as_ref().and_then(|args| args.Pointer().ok()))
                        .flatten();
                    let info = pointer_event_info(&element, args);
                    if let Some(pointer) = pointer {
                        diag::dropped(element.ReleasePointerCapture(&pointer));
                    }
                    if let Some(cb) = &cb {
                        cb.invoke(info);
                    }
                })
                .ok();
        }

        if let Some(cb) = handlers.on_pointer_moved.clone() {
            let element = ui.clone();
            tokens.moved = ui
                .PointerMoved(move |_sender, args| {
                    let info = pointer_event_info(&element, args);
                    cb.invoke(info);
                })
                .ok();
        }

        if let Some(cb) = handlers.on_pointer_entered.clone() {
            let element = ui.clone();
            tokens.entered = ui
                .PointerEntered(move |_sender, args| {
                    let info = pointer_event_info(&element, args);
                    cb.invoke(info);
                })
                .ok();
        }

        if let Some(cb) = handlers.on_pointer_exited.clone() {
            tokens.exited = ui
                .PointerExited(move |_sender, _args| {
                    cb.invoke(());
                })
                .ok();
        }

        if let Some(cb) = handlers.on_pointer_capture_lost.clone() {
            tokens.capture_lost = ui
                .PointerCaptureLost(move |_sender, _args| {
                    cb.invoke(());
                })
                .ok();
        }

        if let Some(cb) = handlers.on_pointer_canceled.clone() {
            tokens.canceled = ui
                .PointerCanceled(move |_sender, _args| {
                    cb.invoke(());
                })
                .ok();
        }

        self.pointer_revokers.borrow_mut().insert(id, tokens);
    }

    fn set_drag_handlers(&mut self, id: ControlId, handlers: Option<&DragHandlers>) {
        let prev = self.drag_revokers.borrow_mut().remove(&id);
        let map = self.controls.borrow();
        let Some(handle) = map.get(&id) else {
            return;
        };
        let ui = handle.as_ui_element();
        drop(prev);

        let Some(handlers) = handlers else {
            return;
        };
        let mut tokens = DragRevokerSet::default();

        if let Some(callback) = handlers.on_drag_enter.clone() {
            let marshaller = WinUIDispatcher::for_current_thread()
                .map(|dispatcher| dispatcher.marshaller())
                .ok();

            tokens.enter = ui
                .DragEnter(move |_sender, args| {
                    let Some(drag_event_args) = args.as_ref() else {
                        return;
                    };

                    let formats = drag_event_args
                        .DataView()
                        .ok()
                        .map(|data_package_view| read_available_formats(&data_package_view))
                        .unwrap_or_default();

                    let agile_deferral = drag_event_args
                        .GetDeferral()
                        .ok()
                        .and_then(|deferral| windows_core::AgileReference::new(&deferral).ok());

                    let agile_args = windows_core::AgileReference::new(drag_event_args).ok();

                    let callback = callback.clone();
                    let marshaller = marshaller.clone();
                    windows_threading::submit(move || {
                        let Some(marshaller) = marshaller else {
                            if let Some(deferral) =
                                agile_deferral.and_then(|agile_ref| agile_ref.resolve().ok())
                            {
                                diag::dropped(deferral.Complete());
                            }
                            return;
                        };
                        dispatch_accept(
                            marshaller,
                            callback,
                            formats,
                            agile_args,
                            agile_deferral,
                            vec![],
                            None,
                        );
                    });
                })
                .ok();
        }

        if let Some(cb) = handlers.on_drag_leave.clone() {
            tokens.leave = ui
                .DragLeave(move |_sender, args| {
                    let ctx = build_drag_context(args.as_ref());
                    cb.call(&ctx);
                })
                .ok();
        }

        if let Some(cb) = handlers.on_drag_over.clone() {
            tokens.over = ui
                .DragOver(move |_sender, args| {
                    accept_or_reject(&cb, args.as_ref());
                })
                .ok();
        }

        if let Some(callback) = handlers.on_drag_drop.clone() {
            let marshaller = WinUIDispatcher::for_current_thread()
                .map(|dispatcher| dispatcher.marshaller())
                .ok();

            tokens.drop = ui
                .Drop(move |_sender, args| {
                    let Some(drag_event_args) = args.as_ref() else {
                        return;
                    };

                    let data_view = drag_event_args.DataView().ok();

                    let formats = data_view
                        .as_ref()
                        .map(read_available_formats)
                        .unwrap_or_default();

                    let agile_deferral = drag_event_args
                        .GetDeferral()
                        .ok()
                        .and_then(|deferral| windows_core::AgileReference::new(&deferral).ok());

                    let agile_data_view = data_view.and_then(|data_package_view| {
                        windows_core::AgileReference::new(&data_package_view).ok()
                    });

                    let agile_args = windows_core::AgileReference::new(drag_event_args).ok();

                    let callback = callback.clone();
                    let marshaller = marshaller.clone();

                    windows_threading::submit(move || {
                        use crate::drag::DroppedItem;

                        let resolved_data_view = agile_data_view
                            .and_then(|agile_reference| agile_reference.resolve().ok());

                        let items: Vec<DroppedItem> = if formats.storage_items {
                            resolved_data_view
                                .as_ref()
                                .and_then(|data_package_view| {
                                    data_package_view.GetStorageItemsAsync().ok()
                                })
                                .and_then(|async_operation| async_operation.join().ok())
                                .map(|v| {
                                    let size = v.Size().unwrap_or(0);
                                    (0..size)
                                        .filter_map(|i| v.GetAt(i).ok())
                                        .map(|item| DroppedItem {
                                            path: item.Path().unwrap_or_default(),
                                            name: item.Name().unwrap_or_default(),
                                            is_folder: item.Attributes().is_ok_and(|attrs| {
                                                attrs.contains(bindings::FileAttributes::Directory)
                                            }),
                                        })
                                        .collect()
                                })
                                .unwrap_or_default()
                        } else {
                            vec![]
                        };

                        let text: Option<String> = if formats.text {
                            resolved_data_view
                                .as_ref()
                                .and_then(|data_package_view| data_package_view.GetTextAsync().ok())
                                .and_then(|async_operation| async_operation.join().ok())
                                .and_then(|h| String::try_from(&h).ok())
                        } else {
                            None
                        };

                        let Some(marshaller) = marshaller else {
                            if let Some(deferral) =
                                agile_deferral.and_then(|agile_ref| agile_ref.resolve().ok())
                            {
                                diag::dropped(deferral.Complete());
                            }
                            return;
                        };
                        dispatch_accept(
                            marshaller,
                            callback,
                            formats,
                            agile_args,
                            agile_deferral,
                            items,
                            text,
                        );
                    });
                })
                .ok();
        }

        self.drag_revokers.borrow_mut().insert(id, tokens);
    }

    fn get_native_element(&self, id: ControlId) -> Option<windows_core::IInspectable> {
        self.get_ui_element(id)
    }
}

const FORMAT_TEXT: &str = "Text";
const FORMAT_HTML: &str = "HTML Format";
const FORMAT_RTF: &str = "Rich Text Format";
const FORMAT_BITMAP: &str = "Bitmap";
const FORMAT_STORAGE_ITEMS: &str = "Shell IDList Array";
const FORMAT_URI_AND_WEB_LINK: &str = "UniformResourceLocatorW";
const FORMAT_APPLICATION_LINK: &str = "ApplicationLink";

#[derive(Copy, Clone, Default)]
struct AvailableFormats {
    text: bool,
    html: bool,
    rtf: bool,
    bitmap: bool,
    storage_items: bool,
    uri: bool,
    web_link: bool,
    application_link: bool,
}

fn read_available_formats(data_package_view: &bindings::DataPackageView) -> AvailableFormats {
    let mut available_formats = AvailableFormats::default();
    let Ok(formats) = data_package_view.AvailableFormats() else {
        return available_formats;
    };

    for s in &formats {
        match s.to_string_lossy().as_str() {
            FORMAT_TEXT => available_formats.text = true,
            FORMAT_HTML => available_formats.html = true,
            FORMAT_RTF => available_formats.rtf = true,
            FORMAT_BITMAP => available_formats.bitmap = true,
            FORMAT_STORAGE_ITEMS => available_formats.storage_items = true,
            FORMAT_URI_AND_WEB_LINK => {
                available_formats.uri = true;
                available_formats.web_link = true;
            }
            FORMAT_APPLICATION_LINK => available_formats.application_link = true,
            _ => {}
        }
    }
    available_formats
}

fn build_drag_context(args: Option<&bindings::DragEventArgs>) -> DragContext {
    use crate::drag::{DragContext, DroppedItem};
    let mut ctx = DragContext {
        has_text: false,
        has_html: false,
        has_rtf: false,
        has_bitmap: false,
        has_storage_items: false,
        has_uri: false,
        has_web_link: false,
        has_application_link: false,
        caption: None,
        glyph_visible: None,
        content_visible: None,
        get_text_fn: None,
        get_storage_items_fn: None,
    };
    let Some(a) = args else { return ctx };
    let Ok(dv) = a.DataView() else {
        return ctx;
    };

    let formats = read_available_formats(&dv);
    ctx.has_text = formats.text;
    ctx.has_html = formats.html;
    ctx.has_rtf = formats.rtf;
    ctx.has_bitmap = formats.bitmap;
    ctx.has_storage_items = formats.storage_items;
    ctx.has_uri = formats.uri;
    ctx.has_web_link = formats.web_link;
    ctx.has_application_link = formats.application_link;

    let dv_text = dv.clone();
    ctx.get_text_fn = Some(Box::new(move || {
        let h = dv_text.GetTextAsync().ok()?.join().ok()?;
        String::try_from(&h).ok()
    }));

    ctx.get_storage_items_fn = Some(Box::new(move || {
        let items = dv.GetStorageItemsAsync().ok().and_then(|op| op.join().ok());
        let Some(items) = items else {
            return Vec::new();
        };
        items
            .into_iter()
            .map(|item| DroppedItem {
                path: item.Path().unwrap_or_default(),
                name: item.Name().unwrap_or_default(),
                is_folder: item
                    .Attributes()
                    .is_ok_and(|a| a.contains(bindings::FileAttributes::Directory)),
            })
            .collect()
    }));

    ctx
}

fn dispatch_accept(
    m: UiMarshaller,
    cb: DragAsyncCallback,
    formats: AvailableFormats,
    iargs_agile: Option<windows_core::AgileReference<bindings::DragEventArgs>>,
    deferral_agile: Option<windows_core::AgileReference<bindings::DragOperationDeferral>>,
    items: Vec<DroppedItem>,
    text: Option<String>,
) {
    use crate::drag::{DragContext, DragOperation};
    m.dispatch(move || {
        let get_storage_items_fn = if items.is_empty() {
            None
        } else {
            let v = items.clone();
            Some(Box::new(move || v.clone()) as Box<dyn Fn() -> Vec<DroppedItem>>)
        };
        let get_text_fn =
            text.map(|t| Box::new(move || Some(t.clone())) as Box<dyn Fn() -> Option<String>>);
        let mut ctx = DragContext {
            has_text: formats.text,
            has_html: formats.html,
            has_rtf: formats.rtf,
            has_bitmap: formats.bitmap,
            has_storage_items: formats.storage_items,
            has_uri: formats.uri,
            has_web_link: formats.web_link,
            has_application_link: formats.application_link,
            caption: None,
            glyph_visible: None,
            content_visible: None,
            get_text_fn,
            get_storage_items_fn,
        };
        let op = cb.call(&mut ctx);
        if let Some(iargs) = iargs_agile.and_then(|a| a.resolve().ok()) {
            let accepted = match op {
                DragOperation::None => bindings::DataPackageOperation::None,
                DragOperation::Copy => bindings::DataPackageOperation::Copy,
                DragOperation::Move => bindings::DataPackageOperation::Move,
                DragOperation::Link => bindings::DataPackageOperation::Link,
            };
            diag::dropped(iargs.SetAcceptedOperation(accepted));
            if (ctx.caption.is_some()
                || ctx.glyph_visible.is_some()
                || ctx.content_visible.is_some())
                && let Ok(ui) = iargs.DragUIOverride()
            {
                if let Some(v) = ctx.caption {
                    diag::dropped(ui.SetCaption(&v));
                }
                if let Some(v) = ctx.glyph_visible {
                    diag::dropped(ui.SetIsGlyphVisible(v));
                }
                if let Some(v) = ctx.content_visible {
                    diag::dropped(ui.SetIsContentVisible(v));
                }
            }
        }
        if let Some(d) = deferral_agile.and_then(|a| a.resolve().ok()) {
            diag::dropped(d.Complete());
        }
    });
}

trait CallAccept {
    fn call(&self, ctx: &mut DragContext) -> DragOperation;
}
impl CallAccept for DragCallback {
    fn call(&self, ctx: &mut DragContext) -> DragOperation {
        self.call(ctx)
    }
}
impl CallAccept for DragAsyncCallback {
    fn call(&self, ctx: &mut DragContext) -> DragOperation {
        self.call(ctx)
    }
}

fn accept_or_reject<C: CallAccept>(cb: &C, args: Option<&bindings::DragEventArgs>) {
    use crate::drag::DragOperation;
    let Some(a) = args else { return };

    let mut ctx = build_drag_context(Some(a));

    let result = cb.call(&mut ctx);

    let accepted = match result {
        DragOperation::None => bindings::DataPackageOperation::None,
        DragOperation::Copy => bindings::DataPackageOperation::Copy,
        DragOperation::Move => bindings::DataPackageOperation::Move,
        DragOperation::Link => bindings::DataPackageOperation::Link,
    };
    diag::dropped(a.SetAcceptedOperation(accepted));

    if (ctx.caption.is_some() || ctx.glyph_visible.is_some() || ctx.content_visible.is_some())
        && let Ok(ui) = a.DragUIOverride()
    {
        if let Some(v) = ctx.caption {
            diag::dropped(ui.SetCaption(&v));
        }
        if let Some(v) = ctx.glyph_visible {
            diag::dropped(ui.SetIsGlyphVisible(v));
        }
        if let Some(v) = ctx.content_visible {
            diag::dropped(ui.SetIsContentVisible(v));
        }
    }
}

/// Extract local/window pointer positions and button state for a pointer callback.
///
/// `element` is captured once at attach time (the handler's own element), so
/// there is no per-event `QueryInterface`: the arg/point/properties classes
/// each `Deref` to their default interface.
fn pointer_event_info(
    element: &bindings::UIElement,
    args: windows_core::InRef<'_, bindings::PointerRoutedEventArgs>,
) -> PointerEventInfo {
    let mut info = PointerEventInfo::default();
    let Some(args) = args.as_ref() else {
        return info;
    };

    if let Ok(point) = args.GetCurrentPoint(element) {
        if let Ok(pos) = point.Position() {
            info.x = pos.x as f64;
            info.y = pos.y as f64;
        }
        if let Ok(props) = point.Properties() {
            info.is_left_button_pressed = props.IsLeftButtonPressed().unwrap_or(false);
            info.is_right_button_pressed = props.IsRightButtonPressed().unwrap_or(false);
            info.is_middle_button_pressed = props.IsMiddleButtonPressed().unwrap_or(false);
        }
    }

    if let Ok(point) = args.GetCurrentPoint(None::<&bindings::UIElement>)
        && let Ok(pos) = point.Position()
    {
        info.window_x = pos.x as f64;
        info.window_y = pos.y as f64;
    }

    info
}

fn map_placement(p: TooltipPlacement) -> bindings::PlacementMode {
    use TooltipPlacement;
    match p {
        TooltipPlacement::Top => bindings::PlacementMode::Top,
        TooltipPlacement::Bottom => bindings::PlacementMode::Bottom,
        TooltipPlacement::Left => bindings::PlacementMode::Left,
        TooltipPlacement::Right => bindings::PlacementMode::Right,
        TooltipPlacement::Mouse => bindings::PlacementMode::Mouse,
    }
}

/// Best-effort static mount for tooltip content. Supports `TextBlock`,
/// linear `StackPanel`, and `Image`; unsupported kinds fall back to a
/// `TextBlock` showing `kind_name()`.
fn mount_static_tooltip_element(el: &Element) -> Option<bindings::UIElement> {
    match el {
        Element::TextBlock(t) => {
            let tb = bindings::TextBlock::new().ok()?;
            tb.SetText(t.text.as_str()).ok()?;
            tb.cast::<bindings::UIElement>().ok()
        }
        Element::StackPanel(s) => {
            let sp = bindings::StackPanel::new().ok()?;
            sp.SetOrientation(s.orientation).ok()?;
            sp.SetSpacing(s.spacing).ok()?;
            let children = sp.cast::<bindings::IPanel>().ok()?.Children().ok()?;
            for child in &s.children {
                if let Some(cui) = mount_static_tooltip_element(child) {
                    diag::dropped(children.Append(&cui));
                }
            }
            sp.cast::<bindings::UIElement>().ok()
        }
        Element::Image(img) => {
            let i = bindings::Image::new().ok()?;
            if let Ok(Some(source)) = build_image_source(&img.source) {
                diag::dropped(i.SetSource(&source));
            }
            i.cast::<bindings::UIElement>().ok()
        }
        _ => {
            // Fallback: surface the kind_name so the developer sees a
            // hint rather than an empty popup.
            let tb = bindings::TextBlock::new().ok()?;
            tb.SetText(el.kind_name()).ok()?;
            tb.cast::<bindings::UIElement>().ok()
        }
    }
}

#[cfg(test)]
mod templated_scroll_tests {
    use super::*;

    fn following_state() -> TemplatedScrollState {
        TemplatedScrollState {
            following_tail: true,
            last_vertical_offset: 600.0,
            tail_threshold: 120.0,
            pending: Some(PreparedTemplatedScroll::Tail),
            ..Default::default()
        }
    }

    #[test]
    fn content_growth_does_not_detach_tail_intent() {
        let mut state = following_state();

        // The row grew after reconcile, but layout has not corrected the
        // viewport yet. Geometry alone must not look like user scrolling.
        observe_templated_view(&mut state, 600.0, 820.0);

        assert!(state.following_tail);
        assert_eq!(state.pending, Some(PreparedTemplatedScroll::Tail));
    }

    #[test]
    fn upward_user_scroll_detaches_and_cancels_tail_retry() {
        let mut state = following_state();

        observe_templated_view(&mut state, 420.0, 820.0);

        assert!(!state.following_tail);
        assert_eq!(state.pending, None);
    }

    #[test]
    fn returning_near_tail_reenables_following_and_acknowledges() {
        let mut state = following_state();
        state.following_tail = false;
        state.last_vertical_offset = 420.0;

        observe_templated_view(&mut state, 710.0, 820.0);

        assert!(state.following_tail);
        assert_eq!(state.pending, None);
    }
}

//! Session headers, onboarding guidance, and transcript cards.

use super::*;


/// Render `lines` inside a border whose inner width is at least `inner_width`.
///
/// This is useful when callers have already clamped their content to a
/// specific width and want the border math centralized here instead of
/// duplicating padding logic in the TUI widgets themselves.
pub(crate) fn with_border_with_inner_width(
    lines: Vec<Line<'static>>,
    inner_width: usize,
) -> Vec<Line<'static>> {
    with_border_internal(lines, Some(inner_width))
}

fn with_border_internal(
    lines: Vec<Line<'static>>,
    forced_inner_width: Option<usize>,
) -> Vec<Line<'static>> {
    let max_line_width = lines
        .iter()
        .map(|line| {
            line.iter()
                .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
                .sum::<usize>()
        })
        .max()
        .unwrap_or(0);
    let content_width = forced_inner_width
        .unwrap_or(max_line_width)
        .max(max_line_width);

    let mut out = Vec::with_capacity(lines.len() + 2);
    let border_inner_width = content_width + 2;
    out.push(vec![format!("╭{}╮", "─".repeat(border_inner_width)).dim()].into());

    for line in lines.into_iter() {
        let used_width: usize = line
            .iter()
            .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
            .sum();
        let span_count = line.spans.len();
        let mut spans: Vec<Span<'static>> = Vec::with_capacity(span_count + 4);
        spans.push(Span::from("│ ").dim());
        spans.extend(line);
        if used_width < content_width {
            spans.push(Span::from(" ".repeat(content_width - used_width)).dim());
        }
        spans.push(Span::from(" │").dim());
        out.push(Line::from(spans));
    }

    out.push(vec![format!("╰{}╯", "─".repeat(border_inner_width)).dim()].into());

    out
}

/// Return the emoji followed by a hair space (U+200A).
/// Using only the hair space avoids excessive padding after the emoji while
/// still providing a small visual gap across terminals.
pub(crate) fn padded_emoji(emoji: &str) -> String {
    format!("{emoji}\u{200A}")
}

#[derive(Debug)]
struct TooltipHistoryCell {
    tip: String,
    cwd: PathBuf,
}

impl TooltipHistoryCell {
    fn new(tip: String, cwd: &Path) -> Self {
        Self {
            tip,
            cwd: cwd.to_path_buf(),
        }
    }
}

impl HistoryCell for TooltipHistoryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let indent = "  ";
        let indent_width = UnicodeWidthStr::width(indent);
        let wrap_width = usize::from(width.max(1))
            .saturating_sub(indent_width)
            .max(1);
        let mut lines: Vec<Line<'static>> = Vec::new();
        append_markdown(
            &format!("**Tip:** {}", self.tip),
            Some(wrap_width),
            Some(self.cwd.as_path()),
            &mut lines,
        );

        prefix_lines(lines, indent.into(), indent.into())
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        vec![Line::from(format!("Tip: {}", self.tip))]
    }
}

#[derive(Debug)]
pub struct SessionInfoCell(CompositeHistoryCell);

impl SessionInfoCell {
    /// The animated welcome header, if this cell leads with one.
    pub(crate) fn animated_header(&self) -> Option<&SessionHeaderHistoryCell> {
        let header = self
            .0
            .parts
            .first()?
            .as_any()
            .downcast_ref::<SessionHeaderHistoryCell>()?;
        header.is_animated().then_some(header)
    }
}

impl HistoryCell for SessionInfoCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        self.0.display_lines(width)
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.0.desired_height(width)
    }

    fn transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
        self.0.transcript_lines(width)
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        self.0.raw_lines()
    }

    fn transcript_animation_tick(&self) -> Option<u64> {
        self.0.transcript_animation_tick()
    }
}

pub(crate) fn new_session_info(
    config: &Config,
    requested_model: &str,
    session: &ThreadSessionState,
    is_first_event: bool,
    tooltip_override: Option<String>,
    auth_plan: Option<PlanType>,
    show_fast_status: bool,
    frame_requester: Option<crate::tui::FrameRequester>,
) -> SessionInfoCell {
    // Header box rendered as history (so it appears at the very top)
    let mut header = SessionHeaderHistoryCell::new(
        session.model.clone(),
        session.reasoning_effort.clone(),
        show_fast_status,
        config.cwd.to_path_buf(),
        CODEX_CLI_VERSION,
    )
    .with_yolo_mode(has_yolo_permissions(
        session.approval_policy,
        &session.permission_profile,
    ));
    if let Some(frame_requester) = frame_requester {
        header = header.with_animation(frame_requester);
    }
    let mut parts: Vec<Box<dyn HistoryCell>> = vec![Box::new(header)];

    if is_first_event {
        // Help lines below the header (new copy and list)
        let help_lines: Vec<Line<'static>> = vec![
            "  To get started, describe a task or try one of these commands:"
                .dim()
                .into(),
            Line::from(""),
            Line::from(vec![
                "  ".into(),
                "/task".into(),
                " - start a new browser task".dim(),
            ]),
            Line::from(vec![
                "  ".into(),
                "/history".into(),
                " - browse previous tasks".dim(),
            ]),
            Line::from(vec![
                "  ".into(),
                "/browser".into(),
                " - change browser backend".dim(),
            ]),
            Line::from(vec![
                "  ".into(),
                "/model".into(),
                " - choose model and provider".dim(),
            ]),
            Line::from(vec![
                "  ".into(),
                "/context".into(),
                " - inspect context window attribution".dim(),
            ]),
        ];

        parts.push(Box::new(PlainHistoryCell { lines: help_lines }));
    } else {
        if config.show_tooltips
            && let Some(tooltips) = tooltip_override
                .or_else(|| tooltips::get_tooltip(auth_plan, show_fast_status))
                .map(|tip| TooltipHistoryCell::new(tip, &config.cwd))
        {
            parts.push(Box::new(tooltips));
        }
        if requested_model != session.model.as_str() {
            let lines = vec![
                "model changed:".magenta().bold().into(),
                format!("requested: {requested_model}").into(),
                format!("used: {}", session.model).into(),
            ];
            parts.push(Box::new(PlainHistoryCell { lines }));
        }
    }

    SessionInfoCell(CompositeHistoryCell { parts })
}

pub(crate) fn is_yolo_mode(config: &Config) -> bool {
    has_yolo_permissions(
        AskForApproval::from(config.permissions.approval_policy.value()),
        &config.permissions.effective_permission_profile(),
    )
}

pub(crate) fn has_yolo_permissions(
    approval_policy: AskForApproval,
    permission_profile: &PermissionProfile,
) -> bool {
    approval_policy == AskForApproval::Never
        && matches!(
            permission_profile,
            PermissionProfile::Disabled
                | PermissionProfile::Managed {
                    file_system: ManagedFileSystemPermissions::Unrestricted,
                    network: NetworkSandboxPolicy::Enabled,
                }
        )
}
#[derive(Debug)]
pub(crate) struct SessionHeaderHistoryCell {
    version: &'static str,
    model: String,
    model_style: Style,
    reasoning_effort: Option<ReasoningEffortConfig>,
    show_fast_status: bool,
    directory: PathBuf,
    yolo_mode: bool,
    /// Drives the orbit-mark drift + click-to-throw spin; `None` renders static.
    /// `Mutex` (not `Cell`) because history cells must be `Send + Sync`.
    animation: Option<(std::sync::Mutex<crate::bu_logo::WelcomeAnim>, crate::tui::FrameRequester)>,
    /// Row offset (relative to the cell's first display line) of the logo block,
    /// so the widget can hit-test mouse clicks against just the logo. Stores
    /// `row + 1`; `0` means "not yet rendered".
    logo_top_row: std::sync::atomic::AtomicU32,
}

impl SessionHeaderHistoryCell {
    pub(crate) fn new(
        model: String,
        reasoning_effort: Option<ReasoningEffortConfig>,
        show_fast_status: bool,
        directory: PathBuf,
        version: &'static str,
    ) -> Self {
        Self::new_with_style(
            model,
            Style::default(),
            reasoning_effort,
            show_fast_status,
            directory,
            version,
        )
    }

    /// Enable the gentle y-axis drift + click-to-throw animation (Browser Use
    /// Terminal look).
    pub(crate) fn with_animation(mut self, frame_requester: crate::tui::FrameRequester) -> Self {
        self.animation = Some((
            std::sync::Mutex::new(crate::bu_logo::WelcomeAnim::new()),
            frame_requester,
        ));
        self
    }

    /// True when this header renders the animated welcome logo.
    pub(crate) fn is_animated(&self) -> bool {
        self.animation.is_some()
    }

    /// Add a random spin impulse to the logo (mouse click/drag).
    pub(crate) fn throw_logo(&self) {
        if let Some((anim, frame_requester)) = &self.animation {
            if let Ok(mut anim) = anim.lock() {
                anim.throw();
            }
            frame_requester.schedule_frame();
        }
    }

    /// Advance the logo physics one frame. Driven every frame from
    /// `ChatWidget::pre_draw_tick` so the logo keeps spinning even while a
    /// popup/modal is open or the header is momentarily off-screen. Returns
    /// true when this header is animated.
    pub(crate) fn tick_animation(&self) -> bool {
        match &self.animation {
            Some((anim, _)) => {
                if let Ok(mut anim) = anim.lock() {
                    anim.tick();
                }
                true
            }
            None => false,
        }
    }

    /// Row offset of the logo block within the cell's display lines (top border
    /// = row 0). `None` until the cell has rendered at least once.
    pub(crate) fn logo_top_row(&self) -> Option<u16> {
        match self.logo_top_row.load(std::sync::atomic::Ordering::Relaxed) {
            0 => None,
            v => Some((v - 1) as u16),
        }
    }

    /// Logo block height in rows.
    pub(crate) fn logo_height() -> u16 {
        crate::bu_logo::LOGO_H as u16
    }

    pub(crate) fn new_with_style(
        model: String,
        model_style: Style,
        reasoning_effort: Option<ReasoningEffortConfig>,
        show_fast_status: bool,
        directory: PathBuf,
        version: &'static str,
    ) -> Self {
        Self {
            version,
            model,
            model_style,
            reasoning_effort,
            show_fast_status,
            directory,
            yolo_mode: false,
            animation: None,
            logo_top_row: std::sync::atomic::AtomicU32::new(0),
        }
    }

    pub(crate) fn with_yolo_mode(mut self, yolo_mode: bool) -> Self {
        self.yolo_mode = yolo_mode;
        self
    }

    fn format_directory(&self, max_width: Option<usize>) -> String {
        Self::format_directory_inner(&self.directory, max_width)
    }

    pub(crate) fn format_directory_inner(directory: &Path, max_width: Option<usize>) -> String {
        let formatted = if let Some(rel) = relativize_to_home(directory) {
            if rel.as_os_str().is_empty() {
                "~".to_string()
            } else {
                format!("~{}{}", std::path::MAIN_SEPARATOR, rel.display())
            }
        } else {
            directory.display().to_string()
        };

        if let Some(max_width) = max_width {
            if max_width == 0 {
                return String::new();
            }
            if UnicodeWidthStr::width(formatted.as_str()) > max_width {
                return crate::text_formatting::center_truncate_path(&formatted, max_width);
            }
        }

        formatted
    }

    fn reasoning_label(&self) -> Option<&str> {
        self.reasoning_effort
            .as_ref()
            .map(ReasoningEffortConfig::as_str)
    }
}

impl HistoryCell for SessionHeaderHistoryCell {
    fn transcript_animation_tick(&self) -> Option<u64> {
        // A monotonically-changing tick invalidates the transcript overlay's
        // cached tail each frame while the logo spins.
        self.animation.as_ref().map(|(anim, _)| {
            anim.lock()
                .map(|a| (a.rx.to_bits() as u64) ^ ((a.ry.to_bits() as u64) << 1))
                .unwrap_or(0)
        })
    }

    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let inner_width = width as usize;
        if inner_width < crate::bu_logo::LOGO_W {
            return Vec::new();
        }

        // Clean, borderless Browser Use welcome: centered animated orbit-mark,
        // the product name, and the shortcuts hint. No box, no metadata clutter
        // (the model lives in the composer status line).
        let center_pad = |content_width: usize| {
            " ".repeat(inner_width.saturating_sub(content_width) / 2)
        };
        let mut lines: Vec<Line<'static>> = Vec::new();
        lines.push(Line::from(""));

        let logo_pad = center_pad(crate::bu_logo::LOGO_W);
        // Record where the logo band starts for mouse hit-testing (stored+1;
        // getter returns value-1). The active cell has a 1-row top inset added
        // by the transcript renderer, which the mouse handler accounts for.
        self.logo_top_row.store(
            lines.len() as u32 + 1,
            std::sync::atomic::Ordering::Relaxed,
        );
        // Render at the current rotation; the physics is advanced from
        // ChatWidget::pre_draw_tick so it keeps spinning even when a popup or
        // modal is open. Static under tests for deterministic snapshots.
        let logo_rows = match &self.animation {
            Some((anim, _)) if !cfg!(test) => anim
                .lock()
                .map(|a| a.render())
                .unwrap_or_else(|_| crate::bu_logo::render_logo_lines()),
            _ => crate::bu_logo::render_logo_lines(),
        };
        for row in logo_rows {
            lines.push(Line::from(vec![
                Span::from(logo_pad.clone()),
                Span::styled(row, crate::theme::accent()),
            ]));
        }

        lines.push(Line::from(""));
        const TITLE: &str = "Browser Use";
        lines.push(Line::from(vec![
            Span::from(center_pad(TITLE.chars().count())),
            Span::styled(TITLE, crate::theme::accent()),
        ]));

        lines.push(Line::from(""));
        const HINT_PREFIX: &str = "press ";
        const HINT_KEY: &str = "/";
        const HINT_SUFFIX: &str = " for commands";
        lines.push(Line::from(vec![
            Span::from(center_pad(
                HINT_PREFIX.chars().count()
                    + HINT_KEY.chars().count()
                    + HINT_SUFFIX.chars().count(),
            )),
            Span::styled(HINT_PREFIX, crate::theme::dim()),
            Span::styled(HINT_KEY, crate::theme::accent()),
            Span::styled(HINT_SUFFIX, crate::theme::dim()),
        ]));
        lines.push(Line::from(""));

        lines
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        let mut lines = vec![
            Line::from(format!("Browser Harness Agent (v{})", self.version)),
            Line::from(format!(
                "model: {}{}",
                self.model,
                self.reasoning_label()
                    .map(|reasoning| format!(" {reasoning}"))
                    .unwrap_or_default()
            )),
            Line::from(format!(
                "directory: {}",
                self.format_directory(/*max_width*/ None)
            )),
        ];
        if self.yolo_mode {
            lines.push(Line::from("permissions: YOLO mode"));
        }
        lines
    }
}

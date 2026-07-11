//! Session headers, onboarding guidance, and transcript cards.

use super::*;

// Track the full terminal width so the splash card grows with the window,
// like the Browser Use Terminal welcome surface.
pub(crate) const SESSION_HEADER_MAX_INNER_WIDTH: usize = usize::MAX;

pub(crate) fn card_inner_width(width: u16, max_inner_width: usize) -> Option<usize> {
    if width < 4 {
        return None;
    }
    let inner_width = std::cmp::min(width.saturating_sub(4) as usize, max_inner_width);
    Some(inner_width)
}

/// Render `lines` inside a border sized to the widest span in the content.
pub(crate) fn with_border(lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    with_border_internal(lines, /*forced_inner_width*/ None)
}

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
    /// Drives the orbit-mark drift animation; `None` renders it static.
    animation: Option<(std::time::Instant, crate::tui::FrameRequester)>,
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

    /// Enable the gentle y-axis drift animation (Browser Use Terminal look).
    pub(crate) fn with_animation(mut self, frame_requester: crate::tui::FrameRequester) -> Self {
        self.animation = Some((std::time::Instant::now(), frame_requester));
        self
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
        self.animation
            .as_ref()
            .map(|(started, _)| started.elapsed().as_millis() as u64 / 70)
    }

    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let Some(inner_width) = card_inner_width(width, SESSION_HEADER_MAX_INNER_WIDTH) else {
            return Vec::new();
        };

        let make_row = |spans: Vec<Span<'static>>| Line::from(spans);

        // Browser Use Terminal splash: centered braille orbit-mark logo, the
        // product name, version, and the shortcuts hint.
        let center_pad = |content_width: usize| {
            " ".repeat(inner_width.saturating_sub(content_width) / 2)
        };
        let mut splash: Vec<Line<'static>> = Vec::new();
        if inner_width >= crate::bu_logo::LOGO_W {
            let logo_pad = center_pad(crate::bu_logo::LOGO_W);
            // Gentle y-axis drift at ~14fps, matching the Browser Use Terminal
            // welcome animation (0.4 rad/s). Static under tests for
            // deterministic snapshots.
            let logo_rows = match &self.animation {
                Some((started, frame_requester)) if !cfg!(test) => {
                    frame_requester.schedule_frame_in(std::time::Duration::from_millis(70));
                    crate::bu_logo::render_logo_lines_at(0.0, started.elapsed().as_secs_f32() * 0.4)
                }
                _ => crate::bu_logo::render_logo_lines(),
            };
            for row in logo_rows {
                splash.push(Line::from(vec![
                    Span::from(logo_pad.clone()),
                    Span::styled(row, crate::theme::accent()),
                ]));
            }
            splash.push(Line::from(""));
        }
        const TITLE: &str = "Browser Use";
        splash.push(Line::from(vec![
            Span::from(center_pad(TITLE.chars().count())),
            Span::from(TITLE).bold(),
        ]));
        let version_label = format!("Browser Harness Agent v{}", self.version);
        splash.push(Line::from(vec![
            Span::from(center_pad(version_label.chars().count())),
            Span::styled(version_label, crate::theme::muted()),
        ]));
        splash.push(Line::from(""));
        const HINT_PREFIX: &str = "press ";
        const HINT_KEY: &str = "/";
        const HINT_SUFFIX: &str = " for shortcuts";
        splash.push(Line::from(vec![
            Span::from(center_pad(
                HINT_PREFIX.chars().count()
                    + HINT_KEY.chars().count()
                    + HINT_SUFFIX.chars().count(),
            )),
            Span::styled(HINT_PREFIX, crate::theme::muted()),
            Span::from(HINT_KEY).bold(),
            Span::styled(HINT_SUFFIX, crate::theme::muted()),
        ]));

        const CHANGE_MODEL_HINT_COMMAND: &str = "/model";
        const CHANGE_MODEL_HINT_EXPLANATION: &str = " to change";
        const DIR_LABEL: &str = "directory:";
        const PERMISSIONS_LABEL: &str = "permissions:";
        let label_width = if self.yolo_mode {
            DIR_LABEL.len().max(PERMISSIONS_LABEL.len())
        } else {
            DIR_LABEL.len()
        };

        let model_label = format!(
            "{model_label:<label_width$}",
            model_label = "model:",
            label_width = label_width
        );
        let reasoning_label = self.reasoning_label();
        let model_spans: Vec<Span<'static>> = {
            let mut spans = vec![
                Span::from(format!("{model_label} ")).dim(),
                Span::styled(self.model.clone(), self.model_style),
            ];
            if let Some(reasoning) = reasoning_label {
                spans.push(Span::from(" "));
                spans.push(Span::from(reasoning.to_owned()));
            }
            if self.show_fast_status {
                spans.push("   ".into());
                spans.push(Span::styled("fast", self.model_style.magenta()));
            }
            spans.push("   ".dim());
            spans.push(CHANGE_MODEL_HINT_COMMAND.cyan());
            spans.push(CHANGE_MODEL_HINT_EXPLANATION.dim());
            spans
        };

        let dir_label = format!("{DIR_LABEL:<label_width$}");
        let dir_prefix = format!("{dir_label} ");
        let dir_prefix_width = UnicodeWidthStr::width(dir_prefix.as_str());
        let dir_max_width = inner_width.saturating_sub(dir_prefix_width);
        let dir = self.format_directory(Some(dir_max_width));
        let dir_spans = vec![Span::from(dir_prefix).dim(), Span::from(dir)];

        let mut lines = splash;
        lines.push(make_row(Vec::new()));
        lines.push(make_row(model_spans));
        lines.push(make_row(dir_spans));

        if self.yolo_mode {
            let permissions_label = format!("{PERMISSIONS_LABEL:<label_width$}");
            lines.push(make_row(vec![
                Span::from(format!("{permissions_label} ")).dim(),
                "YOLO mode".magenta().bold(),
            ]));
        }

        with_border_with_inner_width(lines, inner_width)
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

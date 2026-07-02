use std::collections::HashMap;
use std::time::Instant;

use crate::sync_status::{SyncStatusPeerReport, SyncStatusReport, SyncStatusTrackerReport};

#[derive(Default)]
pub(crate) struct SyncStatusWatchState {
    peers: HashMap<String, SyncStatusWatchPeerState>,
    total_pending_history: Vec<usize>,
    total_rate_history: Vec<f64>,
    frame: usize,
}

#[derive(Debug, Clone)]
struct SyncStatusWatchPeerState {
    last_pull_cursor: i64,
    last_push_cursor: i64,
    last_outbound_push_pending: usize,
    max_outbound_push_pending: usize,
    last_sample: Instant,
}

#[derive(Debug, Clone, Copy)]
struct SyncStatusWatchPeerRates {
    pull_per_sec: f64,
    push_per_sec: f64,
    pending_drain_per_sec: f64,
}

#[derive(Debug, Clone, Copy)]
struct MeshWatchTotals {
    progress: usize,
    pending: usize,
    pull_rate: f64,
    push_rate: f64,
    drain_rate: f64,
}

#[derive(Debug, Clone, Copy)]
struct MeshPoint {
    x: usize,
    y: usize,
}

struct BrailleCanvas {
    width: usize,
    height: usize,
    cells: Vec<u8>,
    overlays: Vec<Option<char>>,
}

impl BrailleCanvas {
    fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            cells: vec![0; width.saturating_mul(height)],
            overlays: vec![None; width.saturating_mul(height)],
        }
    }

    fn set_pixel(&mut self, x: isize, y: isize) {
        if x < 0 || y < 0 {
            return;
        }
        let x = x as usize;
        let y = y as usize;
        let pixel_width = self.width.saturating_mul(2);
        let pixel_height = self.height.saturating_mul(4);
        if x >= pixel_width || y >= pixel_height {
            return;
        }
        let cell_x = x / 2;
        let cell_y = y / 4;
        let bit = match (x % 2, y % 4) {
            (0, 0) => 0x01,
            (0, 1) => 0x02,
            (0, 2) => 0x04,
            (0, 3) => 0x40,
            (1, 0) => 0x08,
            (1, 1) => 0x10,
            (1, 2) => 0x20,
            (1, 3) => 0x80,
            _ => 0,
        };
        let Some(cell) = self.cells.get_mut(cell_y * self.width + cell_x) else {
            return;
        };
        *cell |= bit;
    }

    fn draw_line(&mut self, from: MeshPoint, to: MeshPoint) {
        self.draw_pixel_line(
            from.x.saturating_mul(2).saturating_add(1) as isize,
            from.y.saturating_mul(4).saturating_add(2) as isize,
            to.x.saturating_mul(2).saturating_add(1) as isize,
            to.y.saturating_mul(4).saturating_add(2) as isize,
        );
    }

    fn draw_pixel_line(&mut self, mut x0: isize, mut y0: isize, x1: isize, y1: isize) {
        let dx = (x1 - x0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let dy = -(y1 - y0).abs();
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        loop {
            self.set_pixel(x0, y0);
            if x0 == x1 && y0 == y1 {
                break;
            }
            let e2 = err.saturating_mul(2);
            if e2 >= dy {
                err += dy;
                x0 += sx;
            }
            if e2 <= dx {
                err += dx;
                y0 += sy;
            }
        }
    }

    fn draw_circle(&mut self, center: MeshPoint, radius: usize) {
        let cx = center.x.saturating_mul(2).saturating_add(1) as isize;
        let cy = center.y.saturating_mul(4).saturating_add(2) as isize;
        let radius = radius.max(1) as isize;
        for y in -radius..=radius {
            for x in -radius..=radius {
                if x * x + y * y <= radius * radius {
                    self.set_pixel(cx + x, cy + y);
                }
            }
        }
    }

    fn put_char(&mut self, x: isize, y: isize, value: char) {
        if x < 0 || y < 0 {
            return;
        }
        let x = x as usize;
        let y = y as usize;
        if x >= self.width || y >= self.height {
            return;
        }
        if let Some(slot) = self.overlays.get_mut(y * self.width + x) {
            *slot = Some(value);
        }
    }

    fn put_label(&mut self, x: isize, y: isize, value: &str, max_width: usize) {
        if max_width == 0 {
            return;
        }
        let text = truncate_display(value, max_width);
        for (offset, ch) in text.chars().enumerate() {
            self.put_char(x + offset as isize, y, ch);
        }
    }

    fn lines(&self) -> Vec<String> {
        let mut lines = Vec::with_capacity(self.height);
        for y in 0..self.height {
            let mut line = String::with_capacity(self.width);
            for x in 0..self.width {
                let idx = y * self.width + x;
                if let Some(ch) = self.overlays[idx] {
                    line.push(ch);
                } else if self.cells[idx] == 0 {
                    line.push(' ');
                } else {
                    let value = 0x2800_u32 + u32::from(self.cells[idx]);
                    line.push(char::from_u32(value).unwrap_or(' '));
                }
            }
            lines.push(line);
        }
        lines
    }
}

#[derive(Debug, Clone)]
struct SyncStatusWatchPeerView<'a> {
    peer: &'a SyncStatusPeerReport,
    rates: SyncStatusWatchPeerRates,
    baseline: usize,
    progress: usize,
    peer_name: String,
}

fn sync_status_watch_peer_cmp(
    left: &SyncStatusWatchPeerView<'_>,
    right: &SyncStatusWatchPeerView<'_>,
) -> std::cmp::Ordering {
    sync_status_watch_peer_severity(right)
        .cmp(&sync_status_watch_peer_severity(left))
        .then_with(|| {
            right
                .peer
                .outbound_push_pending
                .cmp(&left.peer.outbound_push_pending)
        })
        .then_with(|| {
            right
                .peer
                .last_seen_age_sec
                .unwrap_or(-1)
                .cmp(&left.peer.last_seen_age_sec.unwrap_or(-1))
        })
        .then_with(|| left.peer_name.cmp(&right.peer_name))
}

fn sync_status_watch_peer_severity(view: &SyncStatusWatchPeerView<'_>) -> u8 {
    let stale = view.peer.last_seen_age_sec.is_some_and(|age| age > 300);
    let queued = view.peer.outbound_push_pending > 0;
    let moving = view.rates.pending_drain_per_sec > 0.0 || view.rates.push_per_sec > 0.0;
    if stale && queued {
        5
    } else if queued && moving {
        4
    } else if queued {
        3
    } else if stale {
        2
    } else if view.rates.pull_per_sec > 0.0 || view.rates.push_per_sec > 0.0 {
        1
    } else {
        0
    }
}

fn sync_status_watch_peer_status(view: &SyncStatusWatchPeerView<'_>) -> &'static str {
    if view.peer.last_seen_age_sec.is_some_and(|age| age > 300) {
        "stale"
    } else if view.peer.outbound_push_pending > 0
        && (view.rates.pending_drain_per_sec > 0.0 || view.rates.push_per_sec > 0.0)
    {
        "sending"
    } else if view.peer.outbound_push_pending > 0 {
        "queued"
    } else if view.rates.pull_per_sec > 0.0 || view.rates.push_per_sec > 0.0 {
        "active"
    } else {
        "ok"
    }
}

pub(crate) fn render_sync_status_watch_frame(
    state: &mut SyncStatusWatchState,
    report: &SyncStatusReport,
    now: Instant,
    frame_width: usize,
) -> String {
    const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    const PANEL_GAP: &str = "  ";
    const TRAFFIC_MIN_WIDTH: usize = 44;
    const TRAFFIC_MAX_WIDTH: usize = 62;

    let width = frame_width.max(80);
    let traffic_width = (width / 3).clamp(TRAFFIC_MIN_WIDTH, TRAFFIC_MAX_WIDTH);
    let mesh_width = width.saturating_sub(traffic_width + display_width(PANEL_GAP));

    let spinner = SPINNER[state.frame % SPINNER.len()];
    state.frame = state.frame.wrapping_add(1);

    let mut out = String::new();
    let total_outbound_pending: usize = report
        .peers
        .iter()
        .map(|peer| peer.outbound_push_pending)
        .sum();
    let peer_views = build_sync_status_watch_peer_views(state, report, now);

    push_watch_line(
        &mut out,
        width,
        &format!(
            "{spinner} rustory sync watch  local={}  head={}  peers={}  to_send={}",
            truncate_display(&report.local_device_id, 24),
            format_count_i64(report.local_head),
            report.peers.len(),
            format_count_usize(total_outbound_pending)
        ),
    );

    out.push('\n');
    let mesh_panel = render_overview_panel(
        report,
        &peer_views,
        report.tracker_status.as_deref(),
        mesh_width,
    );
    let traffic_panel = render_traffic_panel(report, &peer_views, traffic_width);
    for line in join_watch_panels(&mesh_panel, &traffic_panel, PANEL_GAP) {
        push_watch_line(&mut out, width, &line);
    }

    out.push('\n');
    for line in render_link_panel(&peer_views, width) {
        push_watch_line(&mut out, width, &line);
    }

    out.push('\n');
    push_watch_line(
        &mut out,
        width,
        "ctrl+c to exit  •  direct_pull is only this node's direct pull cursor; inbound pushes can still keep data current",
    );
    out
}

fn build_sync_status_watch_peer_views<'a>(
    state: &mut SyncStatusWatchState,
    report: &'a SyncStatusReport,
    now: Instant,
) -> Vec<SyncStatusWatchPeerView<'a>> {
    let mut peer_views = Vec::with_capacity(report.peers.len());
    for peer in &report.peers {
        let rates = sync_status_watch_peer_rates(state, peer, now);
        let baseline = state
            .peers
            .get(&peer.peer_id)
            .map(|state| state.max_outbound_push_pending)
            .unwrap_or(peer.outbound_push_pending);
        let progress = outbound_push_progress_percent(peer.outbound_push_pending, baseline);
        let peer_name = sync_status_peer_display_name(peer);
        peer_views.push(SyncStatusWatchPeerView {
            peer,
            rates,
            baseline,
            progress,
            peer_name,
        });
    }
    peer_views.sort_by(sync_status_watch_peer_cmp);
    peer_views
}

pub(crate) fn render_mesh_watch_frame(
    state: &mut SyncStatusWatchState,
    report: &SyncStatusReport,
    now: Instant,
    frame_width: usize,
    frame_height: usize,
) -> String {
    const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    const PANEL_GAP: &str = "  ";

    let width = frame_width.max(92);
    let height = frame_height.max(24);
    let phase = state.frame;
    let spinner = SPINNER[phase % SPINNER.len()];
    state.frame = state.frame.wrapping_add(1);

    let peer_views = build_sync_status_watch_peer_views(state, report, now);
    let total_pending: usize = peer_views
        .iter()
        .map(|view| view.peer.outbound_push_pending)
        .sum();
    let total_baseline: usize = peer_views.iter().map(|view| view.baseline).sum();
    let total_progress = outbound_push_progress_percent(total_pending, total_baseline);
    let total_pull_rate: f64 = peer_views.iter().map(|view| view.rates.pull_per_sec).sum();
    let total_push_rate: f64 = peer_views.iter().map(|view| view.rates.push_per_sec).sum();
    let total_drain_rate: f64 = peer_views
        .iter()
        .map(|view| view.rates.pending_drain_per_sec.max(0.0))
        .sum();
    let totals = MeshWatchTotals {
        progress: total_progress,
        pending: total_pending,
        pull_rate: total_pull_rate,
        push_rate: total_push_rate,
        drain_rate: total_drain_rate,
    };
    record_mesh_watch_sample(
        state,
        total_pending,
        total_pull_rate + total_push_rate + total_drain_rate,
    );

    let side_width = (width / 3).clamp(44, 64);
    let map_width = width.saturating_sub(side_width + display_width(PANEL_GAP));
    let topology_rows = (height.saturating_mul(45) / 100).clamp(12, 30);

    let mut out = String::new();
    push_watch_line(
        &mut out,
        width,
        &format!(
            "{spinner} rustory mesh watch  local={}  head={}  peers={}  to_send={}",
            truncate_display(&report.local_device_id, 24),
            format_count_i64(report.local_head),
            report.peers.len(),
            format_count_usize(total_pending),
        ),
    );
    push_watch_line(
        &mut out,
        width,
        "local view: peer→local is direct pull; local→peer is accepted push coverage",
    );

    out.push('\n');
    let topology_panel =
        render_mesh_topology_panel(report, &peer_views, map_width, topology_rows, phase);
    let outbox_panel = render_mesh_outbox_panel(
        report,
        &peer_views,
        report.tracker_status.as_deref(),
        state,
        side_width,
        totals,
    );
    for line in join_watch_panels(&topology_panel, &outbox_panel, PANEL_GAP) {
        push_watch_line(&mut out, width, &line);
    }

    out.push('\n');
    let used_lines = out.lines().count();
    let lane_rows = height
        .saturating_sub(used_lines + 5)
        .clamp(4, peer_views.len().max(4));
    for line in render_mesh_lanes_panel(&peer_views, width, lane_rows) {
        push_watch_line(&mut out, width, &line);
    }

    out.push('\n');
    push_watch_line(
        &mut out,
        width,
        "ctrl+c to exit  •  global peer↔peer flow needs future daemon telemetry; this view is this node's measured mesh edge state",
    );
    out
}

fn record_mesh_watch_sample(
    state: &mut SyncStatusWatchState,
    total_pending: usize,
    total_rate: f64,
) {
    const HISTORY_LIMIT: usize = 48;

    state.total_pending_history.push(total_pending);
    if state.total_pending_history.len() > HISTORY_LIMIT {
        state.total_pending_history.remove(0);
    }
    state.total_rate_history.push(total_rate.max(0.0));
    if state.total_rate_history.len() > HISTORY_LIMIT {
        state.total_rate_history.remove(0);
    }
}

fn render_mesh_topology_panel(
    report: &SyncStatusReport,
    peer_views: &[SyncStatusWatchPeerView<'_>],
    panel_width: usize,
    canvas_rows: usize,
    phase: usize,
) -> Vec<String> {
    let inner_width = panel_width.saturating_sub(2);
    let canvas_rows = canvas_rows.clamp(8, 34);
    let mut canvas = BrailleCanvas::new(inner_width, canvas_rows);
    let center = MeshPoint {
        x: inner_width / 2,
        y: canvas_rows / 2,
    };
    let visible = peer_views
        .len()
        .min(mesh_topology_visible_peer_count(inner_width, canvas_rows));

    canvas.draw_circle(center, 3);
    for (idx, view) in peer_views.iter().take(visible).enumerate() {
        let point = mesh_topology_peer_point(idx, visible.max(1), inner_width, canvas_rows);
        canvas.draw_line(center, point);
        canvas.draw_circle(point, mesh_topology_node_radius(view));
        mesh_topology_put_packet(&mut canvas, center, point, view, phase + idx);
    }

    canvas.put_label(
        center.x.saturating_sub(10) as isize,
        center.y as isize,
        &format!(" ◎ {}", truncate_display(&report.local_device_id, 18)),
        24,
    );
    for (idx, view) in peer_views.iter().take(visible).enumerate() {
        let point = mesh_topology_peer_point(idx, visible.max(1), inner_width, canvas_rows);
        mesh_topology_put_peer_label(&mut canvas, point, view, center, inner_width);
    }

    let mut body = canvas.lines();
    let hidden = peer_views.len().saturating_sub(visible);
    if hidden > 0 && !body.is_empty() {
        let row = body.len().saturating_sub(1);
        body[row] = truncate_display(
            &format!("{}+ {hidden} more peers in Flow Lanes", body[row]),
            inner_width,
        );
    }
    if peer_views.is_empty() {
        body.push(center_cell(
            "no peers known yet; start rr daemon on another device",
            inner_width,
        ));
    }

    box_watch_panel("Mesh Topology", panel_width, body)
}

fn mesh_topology_visible_peer_count(width: usize, rows: usize) -> usize {
    let by_width = (width / 13).max(4);
    let by_height = (rows / 2).max(4);
    by_width.min(by_height).clamp(4, 12)
}

fn mesh_topology_peer_point(idx: usize, count: usize, width: usize, rows: usize) -> MeshPoint {
    let center_x = width as f64 / 2.0;
    let center_y = rows as f64 / 2.0;
    let radius_x = (width as f64 / 2.0 - 13.0).max(8.0);
    let radius_y = (rows as f64 / 2.0 - 2.0).max(3.0);
    let angle =
        -std::f64::consts::FRAC_PI_2 + std::f64::consts::TAU * (idx as f64) / (count.max(1) as f64);
    let x = (center_x + radius_x * angle.cos()).round();
    let y = (center_y + radius_y * angle.sin()).round();
    MeshPoint {
        x: (x as isize).clamp(1, width.saturating_sub(2) as isize) as usize,
        y: (y as isize).clamp(1, rows.saturating_sub(2) as isize) as usize,
    }
}

fn mesh_topology_node_radius(view: &SyncStatusWatchPeerView<'_>) -> usize {
    if view.peer.outbound_push_pending >= 1000 {
        3
    } else if view.peer.outbound_push_pending > 0
        || view.rates.pull_per_sec > 0.0
        || view.rates.push_per_sec > 0.0
    {
        2
    } else {
        1
    }
}

fn mesh_topology_put_packet(
    canvas: &mut BrailleCanvas,
    center: MeshPoint,
    peer: MeshPoint,
    view: &SyncStatusWatchPeerView<'_>,
    phase: usize,
) {
    let marker = if view.rates.pull_per_sec > 0.0 || view.rates.push_per_sec > 0.0 {
        '◆'
    } else if view.peer.outbound_push_pending > 0 {
        '◇'
    } else {
        return;
    };
    let steps = 10;
    let mut slot = phase % steps;
    if view.rates.pull_per_sec > view.rates.push_per_sec && view.peer.outbound_push_pending == 0 {
        slot = steps.saturating_sub(1).saturating_sub(slot);
    }
    let t = (slot + 1) as f64 / (steps + 1) as f64;
    let x = center.x as f64 + (peer.x as f64 - center.x as f64) * t;
    let y = center.y as f64 + (peer.y as f64 - center.y as f64) * t;
    canvas.put_char(x.round() as isize, y.round() as isize, marker);
}

fn mesh_topology_put_peer_label(
    canvas: &mut BrailleCanvas,
    point: MeshPoint,
    view: &SyncStatusWatchPeerView<'_>,
    center: MeshPoint,
    width: usize,
) {
    let label = mesh_node_label(view);
    let max_width = (width / 4).clamp(14, 26);
    let y = point.y as isize;
    let raw_x = if point.x <= center.x {
        point.x as isize - max_width as isize - 2
    } else {
        point.x as isize + 2
    };
    let max_x = width.saturating_sub(max_width) as isize;
    canvas.put_label(raw_x.clamp(0, max_x), y, &label, max_width);
}

fn render_mesh_outbox_panel(
    report: &SyncStatusReport,
    peer_views: &[SyncStatusWatchPeerView<'_>],
    trackers: Option<&[SyncStatusTrackerReport]>,
    state: &SyncStatusWatchState,
    panel_width: usize,
    totals: MeshWatchTotals,
) -> Vec<String> {
    let inner_width = panel_width.saturating_sub(2);
    let queued = peer_views
        .iter()
        .filter(|view| view.peer.outbound_push_pending > 0)
        .count();
    let stale = peer_views
        .iter()
        .filter(|view| view.peer.last_seen_age_sec.is_some_and(|age| age > 300))
        .count();
    let active = peer_views
        .iter()
        .filter(|view| view.rates.pull_per_sec > 0.0 || view.rates.push_per_sec > 0.0)
        .count();
    let hottest = peer_views
        .iter()
        .max_by_key(|view| view.peer.outbound_push_pending);
    let oldest_seen = peer_views
        .iter()
        .filter_map(|view| view.peer.last_seen_age_sec)
        .max()
        .map(|age| format!("oldest {}", format_age_sec(age)))
        .unwrap_or_else(|| "oldest unknown".to_string());

    let mut body = vec![
        tracker_summary_line(trackers),
        traffic_kv_line("local", &report.local_device_id, inner_width),
        traffic_kv_line(
            "head",
            &format!(
                "{} rows   {} peers",
                format_count_i64(report.local_head),
                report.peers.len()
            ),
            inner_width,
        ),
        traffic_kv_line("seen", &oldest_seen, inner_width),
        String::new(),
        traffic_progress_line("to_send", totals.progress, totals.pending, inner_width),
        traffic_kv_line(
            "trend",
            &format!(
                "queue {}",
                sparkline_usize(&state.total_pending_history, inner_width.saturating_sub(14))
            ),
            inner_width,
        ),
        traffic_kv_line(
            "rate",
            &format!(
                "pull {}/s  push {}/s  drain {}/s",
                format_rate(totals.pull_rate),
                format_rate(totals.push_rate),
                format_rate(totals.drain_rate),
            ),
            inner_width,
        ),
        traffic_kv_line(
            "spark",
            &format!(
                "flow {}",
                sparkline_f64(&state.total_rate_history, inner_width.saturating_sub(13))
            ),
            inner_width,
        ),
        traffic_kv_line(
            "health",
            &format!("{queued} queued   {active} active   {stale} stale"),
            inner_width,
        ),
    ];

    if let Some(view) = hottest.filter(|view| view.peer.outbound_push_pending > 0) {
        body.push(traffic_kv_line(
            "hot",
            &format!(
                "{}  {} left  {}",
                truncate_display(&view.peer_name, 22),
                format_count_usize(view.peer.outbound_push_pending),
                link_push_progress_line(view.progress, 14),
            ),
            inner_width,
        ));
    }
    if peer_views
        .iter()
        .all(|view| view.peer.outbound_push_pending == 0)
    {
        body.push(truncate_display(
            "steady: no queued local rows",
            inner_width,
        ));
    }

    box_watch_panel("Outbox", panel_width, body)
}

fn render_mesh_lanes_panel(
    peer_views: &[SyncStatusWatchPeerView<'_>],
    panel_width: usize,
    max_rows: usize,
) -> Vec<String> {
    let inner_width = panel_width.saturating_sub(2);
    let mut body = if inner_width >= 132 {
        render_mesh_lanes_wide(peer_views, inner_width, max_rows)
    } else {
        render_mesh_lanes_compact(peer_views, inner_width, max_rows)
    };

    if peer_views.is_empty() {
        body.push("no local mesh edges yet".to_string());
    }

    box_watch_panel("Flow Lanes", panel_width, body)
}

fn render_mesh_lanes_wide(
    peer_views: &[SyncStatusWatchPeerView<'_>],
    inner_width: usize,
    max_rows: usize,
) -> Vec<String> {
    const STATE_COL: usize = 8;
    const SEEN_COL: usize = 8;
    const DIRECT_COL: usize = 11;
    const PULL_RATE_COL: usize = 8;
    const SENT_COL: usize = 11;
    const PENDING_COL: usize = 9;
    const DRAIN_COL: usize = 8;
    const PROGRESS_COL: usize = 24;
    const GAPS: usize = 8;

    let fixed_width = STATE_COL
        + SEEN_COL
        + DIRECT_COL
        + PULL_RATE_COL
        + SENT_COL
        + PENDING_COL
        + DRAIN_COL
        + PROGRESS_COL
        + GAPS;
    let peer_col = inner_width.saturating_sub(fixed_width).max(20);
    let mut body = Vec::new();
    body.push(format!(
        "{} {} {} {} {} {} {} {} {}",
        fit_cell("peer", peer_col),
        fit_cell("state", STATE_COL),
        right_cell("seen", SEEN_COL),
        right_cell("direct", DIRECT_COL),
        right_cell("pull/s", PULL_RATE_COL),
        right_cell("sent", SENT_COL),
        right_cell("to_send", PENDING_COL),
        right_cell("drain/s", DRAIN_COL),
        fit_cell("coverage", PROGRESS_COL),
    ));
    body.push("─".repeat(inner_width));

    for view in peer_views.iter().take(max_rows) {
        body.push(format!(
            "{} {} {} {} {} {} {} {} {}",
            fit_cell(&view.peer_name, peer_col),
            fit_cell(sync_status_watch_peer_status(view), STATE_COL),
            right_cell(&format_age_opt(view.peer.last_seen_age_sec), SEEN_COL),
            right_cell(&format_count_i64(view.peer.pull_cursor), DIRECT_COL),
            right_cell(&format_rate(view.rates.pull_per_sec), PULL_RATE_COL),
            right_cell(&format_count_i64(view.peer.push_cursor), SENT_COL),
            right_cell(
                &format_count_usize(view.peer.outbound_push_pending),
                PENDING_COL,
            ),
            right_cell(
                &format_rate(view.rates.pending_drain_per_sec.max(0.0)),
                DRAIN_COL,
            ),
            link_push_progress_line(view.progress, PROGRESS_COL),
        ));
    }
    append_hidden_peer_count(&mut body, peer_views.len(), max_rows, inner_width);
    body
}

fn render_mesh_lanes_compact(
    peer_views: &[SyncStatusWatchPeerView<'_>],
    inner_width: usize,
    max_rows: usize,
) -> Vec<String> {
    let state_col = 8;
    let seen_col = 7;
    let pending_col = 9;
    let progress_col = 16;
    let variable =
        inner_width.saturating_sub(state_col + seen_col + pending_col + progress_col + 5);
    let peer_col = (variable / 2).clamp(18, 34);
    let cursor_col = variable.saturating_sub(peer_col).max(28);

    let mut body = Vec::new();
    body.push(format!(
        "{} {} {} {} {} {}",
        fit_cell("peer", peer_col),
        fit_cell("state", state_col),
        right_cell("seen", seen_col),
        fit_cell("direct/sent", cursor_col),
        right_cell("to_send", pending_col),
        fit_cell("coverage", progress_col),
    ));
    body.push("─".repeat(inner_width));

    for view in peer_views.iter().take(max_rows) {
        let cursor = format!(
            "direct {}  sent {}",
            format_count_i64(view.peer.pull_cursor),
            format_count_i64(view.peer.push_cursor),
        );
        body.push(format!(
            "{} {} {} {} {} {}",
            fit_cell(&view.peer_name, peer_col),
            fit_cell(sync_status_watch_peer_status(view), state_col),
            right_cell(&format_age_opt(view.peer.last_seen_age_sec), seen_col),
            fit_cell(&cursor, cursor_col),
            right_cell(
                &format_count_usize(view.peer.outbound_push_pending),
                pending_col,
            ),
            link_push_progress_line(view.progress, progress_col),
        ));
    }
    append_hidden_peer_count(&mut body, peer_views.len(), max_rows, inner_width);
    body
}

fn append_hidden_peer_count(body: &mut Vec<String>, total: usize, visible: usize, width: usize) {
    let hidden = total.saturating_sub(visible);
    if hidden > 0 {
        body.push(truncate_display(
            &format!("+ {hidden} more peers hidden by terminal height"),
            width,
        ));
    }
}

fn mesh_node_label(view: &SyncStatusWatchPeerView<'_>) -> String {
    let peer = view.peer;
    let queued = if peer.outbound_push_pending > 0 {
        format!(" {} left", format_count_usize(peer.outbound_push_pending))
    } else {
        String::new()
    };
    format!(
        "{} {}{} {}",
        mesh_peer_symbol(view),
        truncate_display(&mesh_peer_display_name(view), 22),
        queued,
        format_age_opt(peer.last_seen_age_sec),
    )
}

fn mesh_peer_display_name(view: &SyncStatusWatchPeerView<'_>) -> String {
    view.peer
        .peer_device_id
        .clone()
        .unwrap_or_else(|| short_peer_id(&view.peer.peer_id))
}

fn mesh_peer_symbol(view: &SyncStatusWatchPeerView<'_>) -> &'static str {
    if view.peer.last_seen_age_sec.is_some_and(|age| age > 300) {
        "◌"
    } else if view.rates.pull_per_sec > 0.0 || view.rates.push_per_sec > 0.0 {
        "◆"
    } else if view.peer.outbound_push_pending > 0 {
        "◐"
    } else {
        "●"
    }
}

fn center_cell(value: &str, width: usize) -> String {
    let value = truncate_display(value, width);
    let value_width = display_width(&value);
    if value_width >= width {
        return value;
    }
    let left = (width - value_width) / 2;
    let right = width - value_width - left;
    format!("{}{}{}", " ".repeat(left), value, " ".repeat(right))
}

fn format_age_opt(age: Option<i64>) -> String {
    age.map(format_age_sec).unwrap_or_else(|| "-".to_string())
}

fn format_age_sec(age: i64) -> String {
    let age = age.max(0);
    if age >= 86_400 {
        format!("{}d", age / 86_400)
    } else if age >= 3_600 {
        format!("{}h", age / 3_600)
    } else if age >= 60 {
        format!("{}m", age / 60)
    } else {
        format!("{age}s")
    }
}

fn sparkline_usize(values: &[usize], width: usize) -> String {
    let values = values.iter().map(|value| *value as f64).collect::<Vec<_>>();
    sparkline_f64(values.as_slice(), width)
}

fn sparkline_f64(values: &[f64], width: usize) -> String {
    const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    if width == 0 {
        return String::new();
    }
    if values.is_empty() {
        return "·".repeat(width.min(3));
    }

    let sample_count = values.len().min(width);
    let start = values.len().saturating_sub(sample_count);
    let samples = &values[start..];
    let min = samples.iter().copied().fold(f64::INFINITY, f64::min);
    let max = samples.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let span = (max - min).max(0.000_001);
    let mut out = String::new();
    for value in samples {
        let normalized = ((*value - min) / span).clamp(0.0, 1.0);
        let idx = (normalized * (BARS.len() - 1) as f64).round() as usize;
        out.push(BARS[idx.min(BARS.len() - 1)]);
    }
    out
}

fn render_overview_panel(
    report: &SyncStatusReport,
    peer_views: &[SyncStatusWatchPeerView<'_>],
    trackers: Option<&[SyncStatusTrackerReport]>,
    panel_width: usize,
) -> Vec<String> {
    let inner_width = panel_width.saturating_sub(2);
    let total_pending: usize = peer_views
        .iter()
        .map(|view| view.peer.outbound_push_pending)
        .sum();
    let total_baseline: usize = peer_views.iter().map(|view| view.baseline).sum();
    let progress = outbound_push_progress_percent(total_pending, total_baseline);
    let queued = peer_views
        .iter()
        .filter(|view| view.peer.outbound_push_pending > 0)
        .count();
    let stale = peer_views
        .iter()
        .filter(|view| view.peer.last_seen_age_sec.is_some_and(|age| age > 300))
        .count();
    let direct_zero = peer_views
        .iter()
        .filter(|view| view.peer.pull_cursor == 0)
        .count();
    let total_pull_rate: f64 = peer_views.iter().map(|view| view.rates.pull_per_sec).sum();
    let total_push_rate: f64 = peer_views.iter().map(|view| view.rates.push_per_sec).sum();
    let total_drain_rate: f64 = peer_views
        .iter()
        .map(|view| view.rates.pending_drain_per_sec.max(0.0))
        .sum();

    let body = vec![
        tracker_summary_line(trackers),
        traffic_kv_line("local", &report.local_device_id, inner_width),
        traffic_kv_line(
            "head",
            &format!(
                "{} rows   {} peers",
                format_count_i64(report.local_head),
                report.peers.len()
            ),
            inner_width,
        ),
        traffic_kv_line(
            "outbox",
            &format!(
                "{} rows queued across {} peers",
                format_count_usize(total_pending),
                queued
            ),
            inner_width,
        ),
        traffic_progress_line("progress", progress, total_pending, inner_width),
        traffic_kv_line(
            "rates",
            &format!(
                "pull {}/s   push {}/s   drain {}/s",
                format_rate(total_pull_rate),
                format_rate(total_push_rate),
                format_rate(total_drain_rate)
            ),
            inner_width,
        ),
        traffic_kv_line(
            "health",
            &format!("{stale} stale   {direct_zero} direct_pull=0"),
            inner_width,
        ),
        String::new(),
        truncate_display(
            "read: to_send is local rows not yet accepted by that peer",
            inner_width,
        ),
        truncate_display(
            "read: direct_pull is only this node's completed pull cursor",
            inner_width,
        ),
    ];

    box_watch_panel("Overview", panel_width, body)
}

fn render_traffic_panel(
    report: &SyncStatusReport,
    peer_views: &[SyncStatusWatchPeerView<'_>],
    panel_width: usize,
) -> Vec<String> {
    let inner_width = panel_width.saturating_sub(2);
    let total_pending: usize = peer_views
        .iter()
        .map(|view| view.peer.outbound_push_pending)
        .sum();
    let total_baseline: usize = peer_views.iter().map(|view| view.baseline).sum();
    let total_pull_rate: f64 = peer_views.iter().map(|view| view.rates.pull_per_sec).sum();
    let total_push_rate: f64 = peer_views.iter().map(|view| view.rates.push_per_sec).sum();
    let total_drain_rate: f64 = peer_views
        .iter()
        .map(|view| view.rates.pending_drain_per_sec.max(0.0))
        .sum();
    let progress = outbound_push_progress_percent(total_pending, total_baseline);
    let hottest = peer_views
        .iter()
        .max_by_key(|view| view.peer.outbound_push_pending);
    let seen = peer_views
        .iter()
        .filter_map(|view| view.peer.last_seen_age_sec)
        .max()
        .map(|age| format!("oldest {age}s"))
        .unwrap_or_else(|| "unknown".to_string());

    let mut body = vec![
        traffic_kv_line(
            "head",
            &format!(
                "{} rows   {} peers",
                format_count_i64(report.local_head),
                report.peers.len()
            ),
            inner_width,
        ),
        traffic_kv_line("seen", &seen, inner_width),
        String::new(),
        traffic_rate_line("pull", total_pull_rate, inner_width),
        traffic_rate_line("push", total_push_rate, inner_width),
        traffic_rate_line("drain", total_drain_rate, inner_width),
        traffic_progress_line("to_send", progress, total_pending, inner_width),
    ];

    if let Some(view) = hottest {
        body.push(traffic_hot_line(
            &view.peer_name,
            view.peer.pull_cursor,
            view.peer.push_cursor,
            view.peer.outbound_push_pending,
            inner_width,
        ));
    }

    body.push(String::new());
    for view in peer_views
        .iter()
        .filter(|view| view.peer.outbound_push_pending > 0)
        .take(4)
    {
        body.push(traffic_peer_queue_line(view, inner_width));
    }
    if peer_views
        .iter()
        .all(|view| view.peer.outbound_push_pending == 0)
    {
        body.push(truncate_display("no queued local rows", inner_width));
    }

    body.extend([
        String::new(),
        traffic_kv_line("state", "sending drains queued rows", inner_width),
        traffic_kv_line("", "queued waits for an accepted push", inner_width),
        traffic_kv_line(
            "",
            "stale means tracker has not seen peer recently",
            inner_width,
        ),
    ]);

    box_watch_panel("Attention", panel_width, body)
}

fn traffic_kv_line(label: &str, value: &str, inner_width: usize) -> String {
    const LABEL_WIDTH: usize = 8;

    let label = fit_cell(label, LABEL_WIDTH);
    let value_width = inner_width.saturating_sub(LABEL_WIDTH + 1);
    format!("{label} {}", truncate_display(value, value_width))
}

fn traffic_rate_line(label: &str, rate: f64, inner_width: usize) -> String {
    traffic_kv_line(
        label,
        &format!("{}/s", right_cell(&format_rate(rate), 8)),
        inner_width,
    )
}

fn traffic_progress_line(
    label: &str,
    progress: usize,
    pending: usize,
    inner_width: usize,
) -> String {
    let left = format!("{} [{}]", fit_cell(label, 8), progress_bar(progress, 14));
    let right = format!(
        "{} {} left",
        right_cell(&format!("{progress}%"), 5),
        right_cell(&format_count_usize(pending), 7)
    );
    align_left_right(&left, &right, inner_width)
}

fn traffic_hot_line(
    peer_name: &str,
    pull_cursor: i64,
    push_cursor: i64,
    pending: usize,
    inner_width: usize,
) -> String {
    let right = format!(
        "direct {}  sent {}  {} left",
        right_cell(&format_count_i64(pull_cursor), 7),
        right_cell(&format_count_i64(push_cursor), 7),
        right_cell(&format_count_usize(pending), 7)
    );
    let label_width = 9;
    let peer_width = inner_width
        .saturating_sub(label_width)
        .saturating_sub(display_width(&right))
        .saturating_sub(1);
    let left = format!(
        "{} {}",
        fit_cell("hot", 8),
        truncate_display(peer_name, peer_width)
    );
    align_left_right(&left, &right, inner_width)
}

fn traffic_peer_queue_line(view: &SyncStatusWatchPeerView<'_>, inner_width: usize) -> String {
    let peer = view.peer;
    let right = format!(
        "sent {}  {} left",
        right_cell(&format_count_i64(peer.push_cursor), 7),
        right_cell(&format_count_usize(peer.outbound_push_pending), 7)
    );
    let label_width = 9;
    let peer_width = inner_width
        .saturating_sub(label_width)
        .saturating_sub(display_width(&right))
        .saturating_sub(1);
    let left = format!(
        "{} {}",
        fit_cell(sync_status_watch_peer_status(view), 8),
        truncate_display(&view.peer_name, peer_width)
    );
    align_left_right(&left, &right, inner_width)
}

fn align_left_right(left: &str, right: &str, width: usize) -> String {
    let right_width = display_width(right);
    if width <= right_width {
        return truncate_display(right, width);
    }

    let left_width = width.saturating_sub(right_width + 1);
    format!("{} {}", fit_cell(left, left_width), right)
}

fn render_link_panel(
    peer_views: &[SyncStatusWatchPeerView<'_>],
    panel_width: usize,
) -> Vec<String> {
    let inner_width = panel_width.saturating_sub(2);
    let mut body = if inner_width >= 118 {
        render_link_panel_wide(peer_views, inner_width)
    } else {
        render_link_panel_compact(peer_views, inner_width)
    };

    if peer_views.is_empty() {
        body.push("no peers known yet; run rr daemon or p2p-serve + p2p-sync --push".to_string());
    }

    box_watch_panel("Links", panel_width, body)
}

fn render_link_panel_wide(
    peer_views: &[SyncStatusWatchPeerView<'_>],
    inner_width: usize,
) -> Vec<String> {
    const STATE_COL: usize = 8;
    const SEEN_COL: usize = 8;
    const PULL_CURSOR_COL: usize = 11;
    const PULL_RATE_COL: usize = 8;
    const PUSH_CURSOR_COL: usize = 10;
    const PENDING_COL: usize = 8;
    const DRAIN_COL: usize = 8;
    const PROGRESS_COL: usize = 24;
    const COLUMN_GAPS: usize = 8;

    let fixed_width = STATE_COL
        + SEEN_COL
        + PULL_CURSOR_COL
        + PULL_RATE_COL
        + PUSH_CURSOR_COL
        + PENDING_COL
        + DRAIN_COL
        + PROGRESS_COL
        + COLUMN_GAPS;
    let peer_col = inner_width.saturating_sub(fixed_width).max(18);

    let mut body = Vec::new();
    body.push(format!(
        "{} {} {} {} {} {} {} {} {}",
        fit_cell("peer", peer_col),
        fit_cell("state", STATE_COL),
        right_cell("seen", SEEN_COL),
        right_cell("direct", PULL_CURSOR_COL),
        right_cell("pull/s", PULL_RATE_COL),
        right_cell("sent", PUSH_CURSOR_COL),
        right_cell("to_send", PENDING_COL),
        right_cell("drain/s", DRAIN_COL),
        fit_cell("progress", PROGRESS_COL),
    ));
    body.push("─".repeat(inner_width));

    for view in peer_views {
        let peer = view.peer;
        let last_seen = peer
            .last_seen_age_sec
            .map(|age| format!("{age}s"))
            .unwrap_or_else(|| "-".to_string());
        body.push(format!(
            "{} {} {} {} {} {} {} {} {}",
            fit_cell(&view.peer_name, peer_col),
            fit_cell(sync_status_watch_peer_status(view), STATE_COL),
            right_cell(&last_seen, SEEN_COL),
            right_cell(&format_count_i64(peer.pull_cursor), PULL_CURSOR_COL),
            right_cell(&format_rate(view.rates.pull_per_sec), PULL_RATE_COL),
            right_cell(&format_count_i64(peer.push_cursor), PUSH_CURSOR_COL),
            right_cell(&format_count_usize(peer.outbound_push_pending), PENDING_COL),
            right_cell(
                &format_rate(view.rates.pending_drain_per_sec.max(0.0)),
                DRAIN_COL
            ),
            link_push_progress_line(view.progress, PROGRESS_COL),
        ));
    }

    body
}

fn render_link_panel_compact(
    peer_views: &[SyncStatusWatchPeerView<'_>],
    inner_width: usize,
) -> Vec<String> {
    let state_col = 8;
    let seen_col = 7;
    let variable_width = inner_width.saturating_sub(state_col + seen_col + 4);
    let peer_col = (variable_width / 4).clamp(16, 26);
    let pull_col = (variable_width / 4).clamp(18, 26);
    let push_col = variable_width.saturating_sub(peer_col + pull_col).max(32);

    let mut body = Vec::new();
    body.push(format!(
        "{} {} {} {} {}",
        fit_cell("peer", peer_col),
        fit_cell("state", state_col),
        right_cell("seen", seen_col),
        fit_cell("direct_pull", pull_col),
        fit_cell("local_to_peer", push_col)
    ));
    body.push("─".repeat(inner_width));

    for view in peer_views {
        let peer = view.peer;
        let last_seen = peer
            .last_seen_age_sec
            .map(|age| format!("{age}s"))
            .unwrap_or_else(|| "-".to_string());
        let pull = format!(
            "cur {} {}/s",
            format_count_i64(peer.pull_cursor),
            format_rate(view.rates.pull_per_sec)
        );
        let push = format!(
            "sent {} to_send {} {}",
            format_count_i64(peer.push_cursor),
            format_count_usize(peer.outbound_push_pending),
            link_push_progress_line(view.progress, 12),
        );
        body.push(format!(
            "{} {} {} {} {}",
            fit_cell(&view.peer_name, peer_col),
            fit_cell(sync_status_watch_peer_status(view), state_col),
            right_cell(&last_seen, seen_col),
            fit_cell(&pull, pull_col),
            fit_cell(&push, push_col),
        ));
    }

    body
}

fn link_push_progress_line(progress: usize, width: usize) -> String {
    if width < 8 {
        return truncate_display(&format!("{progress}%"), width);
    }
    let bar_width = width.saturating_sub(8).clamp(4, 18);
    let left = format!("[{}]", progress_bar(progress, bar_width));
    let right = right_cell(&format!("{progress}%"), 5);
    align_left_right(&left, &right, width)
}

fn tracker_summary_line(trackers: Option<&[SyncStatusTrackerReport]>) -> String {
    let Some(trackers) = trackers else {
        return "tracker not checked".to_string();
    };
    if trackers.is_empty() {
        return "tracker none configured".to_string();
    }

    let reachable = trackers.iter().filter(|tracker| tracker.reachable).count();
    let detail = if reachable > 0 {
        trackers
            .iter()
            .filter_map(|tracker| tracker.latency_ms)
            .min()
            .map(|latency| format!("{latency}ms"))
            .unwrap_or_else(|| "latency unknown".to_string())
    } else {
        trackers
            .iter()
            .find_map(|tracker| tracker.error.as_deref())
            .map(|error| format!("fail {error}"))
            .unwrap_or_else(|| "fail".to_string())
    };
    let state = if reachable == trackers.len() {
        "ok"
    } else if reachable == 0 {
        "fail"
    } else {
        "degraded"
    };
    let first = trackers
        .first()
        .map(|tracker| tracker.base_url.as_str())
        .unwrap_or("-");
    format!(
        "tracker {state} {reachable}/{} {detail} {}",
        trackers.len(),
        truncate_display(first, 24)
    )
}

fn join_watch_panels(left: &[String], right: &[String], gap: &str) -> Vec<String> {
    let left_width = left
        .first()
        .map(|line| unicode_width::UnicodeWidthStr::width(line.as_str()))
        .unwrap_or(0);
    let right_width = right
        .first()
        .map(|line| unicode_width::UnicodeWidthStr::width(line.as_str()))
        .unwrap_or(0);
    let height = left.len().max(right.len());
    let mut lines = Vec::with_capacity(height);
    for idx in 0..height {
        let left_line = left
            .get(idx)
            .cloned()
            .unwrap_or_else(|| " ".repeat(left_width));
        let right_line = right
            .get(idx)
            .cloned()
            .unwrap_or_else(|| " ".repeat(right_width));
        lines.push(format!(
            "{}{}{}",
            fit_cell(&left_line, left_width),
            gap,
            fit_cell(&right_line, right_width)
        ));
    }
    lines
}

fn box_watch_panel(title: &str, width: usize, body: Vec<String>) -> Vec<String> {
    let width = width.max(4);
    let inner = width.saturating_sub(2);
    let title = format!(" {title} ");
    let title_width = unicode_width::UnicodeWidthStr::width(title.as_str());
    let top_fill = inner.saturating_sub(title_width);
    let mut lines = Vec::with_capacity(body.len() + 2);
    lines.push(format!("┌{title}{}┐", "─".repeat(top_fill)));
    for line in body {
        lines.push(format!("│{}│", fit_cell(&line, inner)));
    }
    lines.push(format!("└{}┘", "─".repeat(inner)));
    lines
}

fn sync_status_watch_peer_rates(
    state: &mut SyncStatusWatchState,
    peer: &SyncStatusPeerReport,
    now: Instant,
) -> SyncStatusWatchPeerRates {
    let Some(previous) = state.peers.get_mut(&peer.peer_id) else {
        state.peers.insert(
            peer.peer_id.clone(),
            SyncStatusWatchPeerState {
                last_pull_cursor: peer.pull_cursor,
                last_push_cursor: peer.push_cursor,
                last_outbound_push_pending: peer.outbound_push_pending,
                max_outbound_push_pending: peer.outbound_push_pending,
                last_sample: now,
            },
        );
        return SyncStatusWatchPeerRates {
            pull_per_sec: 0.0,
            push_per_sec: 0.0,
            pending_drain_per_sec: 0.0,
        };
    };

    let elapsed = now
        .duration_since(previous.last_sample)
        .as_secs_f64()
        .max(0.001);
    let pull_per_sec = peer
        .pull_cursor
        .saturating_sub(previous.last_pull_cursor)
        .max(0) as f64
        / elapsed;
    let push_per_sec = peer
        .push_cursor
        .saturating_sub(previous.last_push_cursor)
        .max(0) as f64
        / elapsed;
    let pending_drain_per_sec =
        previous.last_outbound_push_pending as f64 - peer.outbound_push_pending as f64;
    let pending_drain_per_sec = pending_drain_per_sec / elapsed;

    previous.last_pull_cursor = peer.pull_cursor;
    previous.last_push_cursor = peer.push_cursor;
    previous.last_outbound_push_pending = peer.outbound_push_pending;
    previous.max_outbound_push_pending = previous
        .max_outbound_push_pending
        .max(peer.outbound_push_pending);
    previous.last_sample = now;

    SyncStatusWatchPeerRates {
        pull_per_sec,
        push_per_sec,
        pending_drain_per_sec,
    }
}

fn sync_status_peer_display_name(peer: &SyncStatusPeerReport) -> String {
    let peer_id = short_peer_id(&peer.peer_id);
    if let Some(device_id) = peer.peer_device_id.as_deref() {
        format!("{device_id} {peer_id}")
    } else {
        peer_id
    }
}

fn short_peer_id(peer_id: &str) -> String {
    let prefix = peer_id.chars().take(10).collect::<String>();
    if peer_id.chars().count() <= 10 {
        prefix
    } else {
        format!("{prefix}…")
    }
}

fn outbound_push_progress_percent(pending: usize, baseline: usize) -> usize {
    if pending == 0 {
        return 100;
    }
    if baseline == 0 {
        return 0;
    }
    baseline.saturating_sub(pending).saturating_mul(100) / baseline
}

fn progress_bar(percent: usize, width: usize) -> String {
    let filled = percent.min(100).saturating_mul(width) / 100;
    format!(
        "{}{}",
        "█".repeat(filled),
        "░".repeat(width.saturating_sub(filled))
    )
}

fn format_rate(value: f64) -> String {
    if value.abs() >= 1000.0 {
        format!("{:.1}k", value / 1000.0)
    } else {
        format!("{value:.0}")
    }
}

fn format_count_i64(value: i64) -> String {
    if value < 0 {
        return value.to_string();
    }
    format_count_usize(usize::try_from(value).unwrap_or(usize::MAX))
}

fn format_count_usize(value: usize) -> String {
    if value >= 1_000_000_000 {
        format!("{:.1}B", value as f64 / 1_000_000_000.0)
    } else if value >= 1_000_000 {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}k", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

fn push_watch_line(out: &mut String, width: usize, line: &str) {
    out.push_str(&truncate_display(line, width));
    out.push('\n');
}

fn display_width(value: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(value)
}

fn fit_cell(value: &str, width: usize) -> String {
    let truncated = truncate_display(value, width);
    let current = display_width(truncated.as_str());
    if current >= width {
        truncated
    } else {
        format!("{truncated}{}", " ".repeat(width - current))
    }
}

fn right_cell(value: &str, width: usize) -> String {
    let truncated = truncate_display(value, width);
    let current = display_width(truncated.as_str());
    if current >= width {
        truncated
    } else {
        format!("{}{}", " ".repeat(width - current), truncated)
    }
}

fn truncate_display(value: &str, max_width: usize) -> String {
    if unicode_width::UnicodeWidthStr::width(value) <= max_width {
        return value.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    if max_width == 1 {
        return "…".to_string();
    }

    let mut out = String::new();
    let mut width = 0;
    let ellipsis_width = 1;
    for ch in value.chars() {
        let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + ch_width + ellipsis_width > max_width {
            break;
        }
        out.push(ch);
        width += ch_width;
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_status_watch_progress_helpers_are_stable() {
        assert_eq!(outbound_push_progress_percent(0, 0), 100);
        assert_eq!(outbound_push_progress_percent(50, 100), 50);
        assert_eq!(outbound_push_progress_percent(150, 100), 0);

        assert_eq!(progress_bar(0, 4), "░░░░");
        assert_eq!(progress_bar(50, 4), "██░░");
        assert_eq!(progress_bar(100, 4), "████");

        assert_eq!(format_rate(42.4), "42");
        assert_eq!(format_rate(1200.0), "1.2k");
    }

    #[test]
    fn sync_status_watch_frame_stays_bounded_for_long_values() {
        let mut state = SyncStatusWatchState::default();
        let report = SyncStatusReport {
            local_head: 2_129_846,
            local_device_id: "user-arm64-with-an-extra-long-local-device-id".to_string(),
            peers: vec![
                SyncStatusPeerReport {
                    peer_id: "12D3KooWE3u4VEsbCGR7w53rbBYi1mZ3kADAgAhDYTj8ACiPBC1M".to_string(),
                    peer_device_id: Some(
                        "sample-node-x86_64-with-a-very-long-device-name".to_string(),
                    ),
                    pull_cursor: 1_526_049,
                    push_cursor: 1_968_089,
                    outbound_push_pending: 2_311,
                    pending_push: 2_311,
                    last_seen_unix: Some(1),
                    last_seen_age_sec: Some(7),
                },
                SyncStatusPeerReport {
                    peer_id: "12D3KooWKvNkdisp13vqjrzZtPkDUz1aB2uVYpWBQCDVT3ihPcJU".to_string(),
                    peer_device_id: Some("node3".to_string()),
                    pull_cursor: 1_818_365,
                    push_cursor: 2_122_722,
                    outbound_push_pending: 0,
                    pending_push: 0,
                    last_seen_unix: Some(1),
                    last_seen_age_sec: Some(123_456),
                },
            ],
            tracker_status: Some(vec![SyncStatusTrackerReport {
                base_url: "https://tracker.example.com/with/a/long/path".to_string(),
                reachable: false,
                latency_ms: None,
                error: Some("timeout: connect with a long transport error message".to_string()),
            }]),
        };

        let frame = render_sync_status_watch_frame(&mut state, &report, Instant::now(), 160);

        assert!(frame.contains("rustory sync watch"));
        assert!(frame.contains("Overview"));
        assert!(frame.contains("Attention"));
        assert!(frame.contains("Links"));
        assert!(frame.contains("direct"));
        assert!(frame.contains("pull/s"));
        assert!(frame.contains("sent"));
        assert!(frame.contains("to_send"));
        assert!(frame.contains("drain/s"));
        assert!(frame.contains("2.3k"));
        assert!(frame.contains("2.0M"));
        assert!(frame.contains("direct_pull is only"));
        assert!(!frame.contains("Mesh Map"));
        for line in frame.lines() {
            let width = unicode_width::UnicodeWidthStr::width(line);
            assert!(width <= 160, "line width {width}: {line}");
        }
    }

    #[test]
    fn mesh_watch_frame_uses_visual_local_mesh_dashboard() {
        let mut state = SyncStatusWatchState::default();
        let report = SyncStatusReport {
            local_head: 4_494_530,
            local_device_id: "user-arm64-with-an-extra-long-local-device-id".to_string(),
            peers: vec![
                SyncStatusPeerReport {
                    peer_id: "12D3KooWE3u4VEsbCGR7w53rbBYi1mZ3kADAgAhDYTj8ACiPBC1M".to_string(),
                    peer_device_id: Some("sample-node-x86_64".to_string()),
                    pull_cursor: 4_400_747,
                    push_cursor: 4_494_530,
                    outbound_push_pending: 0,
                    pending_push: 0,
                    last_seen_unix: Some(1),
                    last_seen_age_sec: Some(27),
                },
                SyncStatusPeerReport {
                    peer_id: "12D3KooWJSi7WKtoW8wp2MnxhheB3Y62fAN9FRHGMspc5fQZfZnH".to_string(),
                    peer_device_id: Some("samplex-x86_64-with-long-name".to_string()),
                    pull_cursor: 0,
                    push_cursor: 3_678_638,
                    outbound_push_pending: 158_485,
                    pending_push: 158_485,
                    last_seen_unix: Some(1),
                    last_seen_age_sec: Some(602),
                },
                SyncStatusPeerReport {
                    peer_id: "12D3KooWKvNkdisp13vqjrzZtPkDUz1aB2uVYpWBQCDVT3ihPcJU".to_string(),
                    peer_device_id: Some("node3-x86_64".to_string()),
                    pull_cursor: 4_345_857,
                    push_cursor: 4_494_530,
                    outbound_push_pending: 2,
                    pending_push: 2,
                    last_seen_unix: Some(1),
                    last_seen_age_sec: Some(3),
                },
            ],
            tracker_status: Some(vec![SyncStatusTrackerReport {
                base_url: "https://tracker.example.com".to_string(),
                reachable: true,
                latency_ms: Some(26),
                error: None,
            }]),
        };

        let frame = render_mesh_watch_frame(&mut state, &report, Instant::now(), 160, 36);

        assert!(frame.contains("rustory mesh watch"));
        assert!(frame.contains("Mesh Topology"));
        assert!(frame.contains("Outbox"));
        assert!(frame.contains("Flow Lanes"));
        assert!(frame.contains("local view: peer"));
        assert!(frame.contains("global peer"));
        assert!(frame.contains("to_send"));
        assert!(frame.contains("coverage"));
        assert!(frame.contains("queue"));
        assert!(frame.contains("158.5k"));
        assert!(!frame.contains("Mesh Map"));
        assert!(!frame.contains("Peer Ring"));
        assert!(
            frame
                .chars()
                .any(|ch| ('\u{2801}'..='\u{28ff}').contains(&ch)),
            "mesh topology should use braille canvas lines: {frame}"
        );
        for line in frame.lines() {
            let width = unicode_width::UnicodeWidthStr::width(line);
            assert!(width <= 160, "line width {width}: {line}");
        }
    }

    #[test]
    fn sync_status_watch_status_lines_align_right_columns() {
        let backlog = traffic_progress_line("to_send", 5, 16, 52);
        let hot = traffic_hot_line(
            "node0 12D3KooWQJ8wUaWhMxSGwGD65PsQFoYaR",
            1_526_049,
            1_968_089,
            13,
            52,
        );
        let progress = link_push_progress_line(66, 24);

        assert_eq!(unicode_width::UnicodeWidthStr::width(backlog.as_str()), 52);
        assert_eq!(unicode_width::UnicodeWidthStr::width(hot.as_str()), 52);
        assert_eq!(unicode_width::UnicodeWidthStr::width(progress.as_str()), 24);
        assert!(backlog.ends_with("   5%      16 left"));
        assert!(hot.contains("direct"));
        assert!(hot.contains("sent"));
        assert!(hot.ends_with("13 left"));
        assert!(progress.ends_with("  66%"));
    }
}

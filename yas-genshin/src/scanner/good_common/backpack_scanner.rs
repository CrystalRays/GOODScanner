use anyhow::Result;
use image::RgbImage;
use log::{error, info};
use regex::Regex;

use yas::ocr::ImageToText;
use yas::utils;

use super::constants::*;
use super::game_controller::{color_distance, GenshinGameController};

/// What the scan callback should do after processing an item.
pub enum ScanAction {
    /// Continue scanning.
    Continue,
    /// Stop scanning immediately.
    Stop,
}

/// Events delivered to the scan callback.
pub enum GridEvent<'a> {
    /// An item was clicked and captured: (item_index, captured_image).
    Item(usize, &'a RgbImage),
    /// A page scroll just completed (useful for clearing per-page state).
    PageScrolled,
}

/// Configuration for backpack grid scanning delays.
pub struct BackpackScanConfig {
    pub delay_grid_item: u64,
    pub delay_scroll: u64,
}

/// Result of a single-row scroll attempt.
#[derive(Debug)]
enum ScrollResult {
    /// Scroll completed: flag pixel changed and returned to initial.
    Success,
    /// Scroll exceeded time/tick limit without completing.
    TimeLimitExceeded,
    /// Used bulk estimation instead of per-row detection.
    Estimated,
    /// User interrupted (RMB down).
    Interrupted,
}

/// Panel pool rect — region of the detail panel whose pixel sum changes
/// when a different item is selected.
/// Located in the center-right area where item stats are displayed.
const PANEL_POOL_RECT: (f64, f64, f64, f64) = (1400.0, 300.0, 300.0, 200.0);

/// Default timeout for panel-load detection (milliseconds).
const PANEL_LOAD_TIMEOUT_MS: u64 = 800;

/// Flag pixel position — a point in the grid area whose color changes when
/// the grid scrolls (top-left of the first grid item).
const SCROLL_FLAG_POS: (f64, f64) = (GRID_FIRST_X, GRID_FIRST_Y);

/// Color distance threshold for detecting flag pixel changes.
const FLAG_COLOR_THRESHOLD: usize = 10;

/// Maximum scroll ticks to attempt for a single row before giving up.
const MAX_SCROLL_TICKS: i32 = 25;

/// After this many successful scroll measurements, switch to bulk estimation.
const SCROLL_ESTIMATION_THRESHOLD: u32 = 5;

/// Reusable backpack grid scanner.
///
/// Encapsulates the shared pattern of navigating a backpack grid (used by
/// weapon and artifact scanners). Provides panel-load detection and adaptive
/// scrolling ported from YAS's `GenshinRepositoryScanController`.
///
/// Usage:
/// ```ignore
/// let mut bp = BackpackScanner::new(&mut controller);
/// bp.open_backpack(1000);
/// bp.select_tab("weapon", 500);
/// let (_, total) = bp.read_item_count(ocr)?;
/// bp.scan_grid(total, &config, |event| { match event { ... } });
/// ```
pub struct BackpackScanner<'a> {
    ctrl: &'a mut GenshinGameController,

    // Adaptive scroll state (ported from YAS controller)
    scrolled_rows: u32,
    avg_scroll_one_row: f64,
    initial_flag_color: image::Rgb<u8>,
}

impl<'a> BackpackScanner<'a> {
    pub fn new(ctrl: &'a mut GenshinGameController) -> Self {
        Self {
            ctrl,
            scrolled_rows: 0,
            avg_scroll_one_row: 0.0,
            initial_flag_color: image::Rgb([0, 0, 0]),
        }
    }

    /// Access the controller's scaler (useful for cloning before scan_grid).
    pub fn scaler(&self) -> &super::coord_scaler::CoordScaler {
        &self.ctrl.scaler
    }

    /// Open the backpack by pressing B.
    pub fn open_backpack(&mut self, delay: u64) {
        self.ctrl.key_press(enigo::Key::Layout('b'));
        utils::sleep(delay as u32);
    }

    /// Select a backpack tab by clicking its position.
    pub fn select_tab(&mut self, tab: &str, delay: u64) {
        let (bx, by) = match tab {
            "weapon" => TAB_WEAPON,
            "artifact" => TAB_ARTIFACT,
            _ => {
                error!("[backpack] unknown tab: {}", tab);
                return;
            }
        };
        self.ctrl.click_at(bx, by);
        utils::sleep(delay as u32);
    }

    /// Read the item count from the backpack header ("X/Y" format).
    pub fn read_item_count(
        &self,
        ocr_model: &dyn ImageToText<RgbImage>,
    ) -> Result<(i32, i32)> {
        let text = self.ctrl.ocr_region(ocr_model, ITEM_COUNT_RECT)?;
        let re = Regex::new(r"(\d+)\s*/\s*(\d+)")?;
        if let Some(caps) = re.captures(&text) {
            let current: i32 = caps[1].parse().unwrap_or(0);
            let total: i32 = caps[2].parse().unwrap_or(0);
            Ok((current, total))
        } else {
            Ok((0, 0))
        }
    }

    /// Sample the flag pixel color (used as baseline for scroll detection).
    fn sample_flag_color(&mut self) {
        if let Ok(color) = self.ctrl.get_flag_color(SCROLL_FLAG_POS.0, SCROLL_FLAG_POS.1) {
            self.initial_flag_color = color;
        }
    }

    /// Scroll one row using adaptive flag-pixel detection.
    ///
    /// Port of `scroll_one_row` from YAS controller.rs:291-322.
    /// Monitors the flag pixel: detects color change (state=1) then
    /// return to initial color (scroll complete).
    fn scroll_one_row_adaptive(&mut self, delay: u64) -> ScrollResult {
        let mut state = 0;
        let mut tick_count = 0;

        while tick_count < MAX_SCROLL_TICKS {
            if utils::is_rmb_down() {
                return ScrollResult::Interrupted;
            }

            self.ctrl.mouse_scroll(-1);
            utils::sleep(delay.min(200) as u32);
            tick_count += 1;

            let color = match self.ctrl.get_flag_color(SCROLL_FLAG_POS.0, SCROLL_FLAG_POS.1) {
                Ok(c) => c,
                Err(_) => return ScrollResult::TimeLimitExceeded,
            };

            if state == 0 && color_distance(&self.initial_flag_color, &color) > FLAG_COLOR_THRESHOLD {
                state = 1;
            } else if state == 1 && color_distance(&self.initial_flag_color, &color) <= FLAG_COLOR_THRESHOLD {
                self.update_avg_scroll(tick_count);
                return ScrollResult::Success;
            }
        }

        ScrollResult::TimeLimitExceeded
    }

    /// Scroll multiple rows, using estimation after enough measurements.
    ///
    /// Port of `scroll_rows` from YAS controller.rs:324-353.
    fn scroll_rows_adaptive(&mut self, count: usize, delay: u64) -> ScrollResult {
        // After enough measurements, use bulk estimation
        if self.scrolled_rows >= SCROLL_ESTIMATION_THRESHOLD {
            let estimated_ticks = self.estimate_scroll_ticks(count as i32);
            for _ in 0..estimated_ticks {
                self.ctrl.mouse_scroll(-1);
            }
            utils::sleep(delay as u32);
            self.align_after_scroll(delay);
            return ScrollResult::Estimated;
        }

        // Otherwise, scroll row-by-row with detection
        for _ in 0..count {
            match self.scroll_one_row_adaptive(delay) {
                ScrollResult::Success | ScrollResult::Estimated => continue,
                ScrollResult::Interrupted => return ScrollResult::Interrupted,
                other => {
                    error!("[backpack] scroll failed: {:?}", other);
                    return other;
                }
            }
        }

        ScrollResult::Success
    }

    /// Try to realign after bulk scrolling by checking the flag pixel.
    fn align_after_scroll(&mut self, delay: u64) {
        for _ in 0..10 {
            let color = match self.ctrl.get_flag_color(SCROLL_FLAG_POS.0, SCROLL_FLAG_POS.1) {
                Ok(c) => c,
                Err(_) => return,
            };

            if color_distance(&self.initial_flag_color, &color) > FLAG_COLOR_THRESHOLD {
                self.ctrl.mouse_scroll(-1);
                utils::sleep(delay.min(200) as u32);
            } else {
                break;
            }
        }
    }

    /// Update running average of scroll ticks per row.
    fn update_avg_scroll(&mut self, tick_count: i32) {
        let total = self.avg_scroll_one_row * self.scrolled_rows as f64 + tick_count as f64;
        self.scrolled_rows += 1;
        self.avg_scroll_one_row = total / self.scrolled_rows as f64;
        info!(
            "[backpack] avg scroll per row: {:.1} ({} measurements)",
            self.avg_scroll_one_row, self.scrolled_rows
        );
    }

    /// Estimate scroll ticks for a given number of rows.
    fn estimate_scroll_ticks(&self, row_count: i32) -> i32 {
        ((self.avg_scroll_one_row * row_count as f64) - 2.0)
            .round()
            .max(0.0) as i32
    }

    /// Main grid traversal with panel-load detection and adaptive scrolling.
    ///
    /// For each item: clicks the grid position, waits for panel to load
    /// (pixel pool detection), captures the game screen, and delivers a
    /// `GridEvent::Item` to the callback.
    ///
    /// After each page scroll, delivers `GridEvent::PageScrolled` (useful for
    /// clearing per-page state like row dedup caches).
    ///
    /// The callback returns `ScanAction::Continue` or `ScanAction::Stop`.
    /// For `GridEvent::PageScrolled`, the return value is ignored.
    ///
    /// If `start_at > 0`, skips to that item index by scrolling pages
    /// and starting from the correct grid position.
    pub fn scan_grid<F>(
        &mut self,
        total: usize,
        config: &BackpackScanConfig,
        start_at: usize,
        mut callback: F,
    ) where
        F: FnMut(GridEvent) -> ScanAction,
    {
        let items_per_page = GRID_COLS * GRID_ROWS;
        let page_count = (total + items_per_page - 1) / items_per_page;

        // Calculate which page and position to start from
        let start_page = start_at / items_per_page;
        let start_offset_in_page = start_at % items_per_page;
        let start_row_in_page = start_offset_in_page / GRID_COLS;
        let start_col_in_page = start_offset_in_page % GRID_COLS;

        let mut item_index = start_at;

        // Sample initial flag color for scroll detection
        self.sample_flag_color();

        // Skip pages by scrolling
        if start_page > 0 {
            info!(
                "[backpack] jumping to item {} (page {}, row {}, col {})",
                start_at, start_page, start_row_in_page, start_col_in_page
            );

            self.ctrl.move_to(GRID_FIRST_X, GRID_FIRST_Y);
            utils::sleep(100);

            let rows_to_skip = start_page * GRID_ROWS;
            match self.scroll_rows_adaptive(rows_to_skip, config.delay_scroll) {
                ScrollResult::Interrupted => return,
                ScrollResult::TimeLimitExceeded => {
                    error!("[backpack] scroll timeout while skipping pages");
                    return;
                }
                _ => {}
            }

            self.sample_flag_color();
            utils::sleep(300);
        }

        'outer: for page in start_page..page_count {
            let mut start_row = 0;
            let remaining = total.saturating_sub(page * items_per_page);

            if remaining < items_per_page {
                let row_count = (remaining + GRID_COLS - 1) / GRID_COLS;
                start_row = GRID_ROWS.saturating_sub(row_count);
                info!(
                    "[backpack] last page: remaining={} rows={} startRow={} page={}/{}",
                    remaining, row_count, start_row, page, page_count
                );
            }

            for row in start_row..GRID_ROWS {
                for col in 0..GRID_COLS {
                    if item_index >= total || utils::is_rmb_down() {
                        break 'outer;
                    }

                    // On the first page after skipping, start from the right position
                    if page == start_page && (row < start_row_in_page
                        || (row == start_row_in_page && col < start_col_in_page))
                    {
                        item_index += 1;
                        continue;
                    }

                    // Click the grid item
                    let x = GRID_FIRST_X + col as f64 * GRID_OFFSET_X;
                    let y = GRID_FIRST_Y + row as f64 * GRID_OFFSET_Y;
                    self.ctrl.move_to(x, y);
                    utils::sleep((config.delay_grid_item / 3).max(1) as u32);
                    self.ctrl.click_at(x, y);

                    // Wait for panel to load (YAS-style pixel pool detection)
                    let _ = self.ctrl.wait_until_panel_loaded(
                        PANEL_POOL_RECT,
                        PANEL_LOAD_TIMEOUT_MS,
                    );

                    // Capture and process
                    let image = match self.ctrl.capture_game() {
                        Ok(img) => img,
                        Err(e) => {
                            error!("[backpack] capture failed: {}", e);
                            item_index += 1;
                            continue;
                        }
                    };

                    match callback(GridEvent::Item(item_index, &image)) {
                        ScanAction::Continue => {}
                        ScanAction::Stop => break 'outer,
                    }

                    item_index += 1;
                }
            }

            // Scroll to next page (unless this is the last page)
            if page < page_count - 1 {
                self.ctrl.move_to(GRID_FIRST_X, GRID_FIRST_Y);
                utils::sleep(100);

                let scroll_rows = GRID_ROWS;
                match self.scroll_rows_adaptive(scroll_rows, config.delay_scroll) {
                    ScrollResult::Interrupted => break 'outer,
                    ScrollResult::TimeLimitExceeded => {
                        error!("[backpack] scroll timeout, stopping");
                        break 'outer;
                    }
                    _ => {}
                }

                // Re-sample flag color after scroll
                self.sample_flag_color();

                callback(GridEvent::PageScrolled);
            }
        }
    }
}

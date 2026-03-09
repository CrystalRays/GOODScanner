use std::rc::Rc;
use std::time::SystemTime;

use anyhow::{bail, Result};
use image::{GenericImageView, RgbImage};
use log::{error, info, warn};
use regex::Regex;

use yas::ocr::ImageToText;

use super::GoodArtifactScannerConfig;
use crate::scanner::good_common::backpack_scanner::{BackpackScanConfig, BackpackScanner, GridEvent, ScanAction};
use crate::scanner::good_common::constants::*;
use crate::scanner::good_common::coord_scaler::CoordScaler;
use crate::scanner::good_common::fuzzy_match::fuzzy_match_map;
use crate::scanner::good_common::game_controller::GenshinGameController;
use crate::scanner::good_common::mappings::MappingManager;
use crate::scanner::good_common::models::{DebugOcrField, DebugScanResult, GoodArtifact, GoodSubStat};
use crate::scanner::good_common::ocr_factory;
use crate::scanner::good_common::pixel_utils;
use crate::scanner::good_common::stat_parser;

/// Computed OCR regions for artifact card (at 1920x1080 base).
/// Port of the ARTIFACT_OCR calculation from GOODScanner/lib/artifact_scanner.js
struct ArtifactOcrRegions {
    part_name: (f64, f64, f64, f64),
    main_stat: (f64, f64, f64, f64),
    level: (f64, f64, f64, f64),
    substats: (f64, f64, f64, f64),
    set_name_x: f64,
    set_name_w: f64,
    set_name_base_y: f64,
    set_name_h: f64,
    equip: (f64, f64, f64, f64),
    elixir: (f64, f64, f64, f64),
}

impl ArtifactOcrRegions {
    fn new() -> Self {
        let card_x: f64 = 1307.0;
        let card_y: f64 = 119.0;
        let card_w: f64 = 494.0;
        let card_h: f64 = 841.0;

        Self {
            part_name: (
                card_x + (card_w * 0.0405).round(),
                card_y + (card_h * 0.0772).round(),
                (card_w * 0.4757).round(),
                (card_h * 0.0475).round(),
            ),
            main_stat: (
                card_x + (card_w * 0.0405).round(),
                card_y + (card_h * 0.1722).round(),
                (card_w * 0.4555).round(),
                (card_h * 0.0416).round(),
            ),
            level: (
                card_x + (card_w * 0.0506).round(),
                card_y + (card_h * 0.3634).round(),
                (card_w * 0.1417).round(),
                (card_h * 0.0416).round(),
            ),
            substats: (1353.0, 475.0, 247.0, 150.0),
            set_name_x: 1330.0,
            set_name_w: 200.0,
            set_name_base_y: 630.0,
            set_name_h: 30.0,
            equip: (
                card_x + (card_w * 0.10).round(),
                card_y + (card_h * 0.935).round(),
                (card_w * 0.85).round(),
                (card_h * 0.06).round(),
            ),
            elixir: (1360.0, 410.0, 140.0, 26.0),
        }
    }
}

/// Result of scanning a single artifact
enum ArtifactScanResult {
    Artifact(GoodArtifact),
    Stop,
    Skip,
}

/// Artifact scanner ported from GOODScanner/lib/artifact_scanner.js.
///
/// Features elixir detection with Y-shift, astral marks, unactivated substats,
/// row-level deduplication, and post-processing filters.
///
/// The scanner holds only business logic (OCR model, mappings, config).
/// The game controller is passed to `scan()` to avoid borrow checker conflicts
/// with `BackpackScanner`.
pub struct GoodArtifactScanner {
    config: GoodArtifactScannerConfig,
    ocr_model: Box<dyn ImageToText<RgbImage> + Send>,
    mappings: Rc<MappingManager>,
    ocr_regions: ArtifactOcrRegions,
}

impl GoodArtifactScanner {
    pub fn new(
        config: GoodArtifactScannerConfig,
        mappings: Rc<MappingManager>,
    ) -> Result<Self> {
        let ocr_model = ocr_factory::create_ocr_model(&config.ocr_backend)?;

        Ok(Self {
            config,
            ocr_model,
            mappings,
            ocr_regions: ArtifactOcrRegions::new(),
        })
    }
}

impl GoodArtifactScanner {
    /// OCR a sub-region of a captured game image.
    fn ocr_image_region(
        &self,
        image: &RgbImage,
        rect: (f64, f64, f64, f64),
        scaler: &CoordScaler,
    ) -> Result<String> {
        let (bx, by, bw, bh) = rect;
        let x = scaler.x(bx) as u32;
        let y = scaler.y(by) as u32;
        let w = scaler.x(bw) as u32;
        let h = scaler.y(bh) as u32;

        let x = x.min(image.width().saturating_sub(1));
        let y = y.min(image.height().saturating_sub(1));
        let w = w.min(image.width().saturating_sub(x));
        let h = h.min(image.height().saturating_sub(y));

        if w == 0 || h == 0 {
            return Ok(String::new());
        }

        let sub = image.view(x, y, w, h).to_image();
        let text = self.ocr_model.image_to_text(&sub, false)?;
        Ok(text.trim().to_string())
    }

    /// OCR a sub-region with Y-offset for elixir artifacts.
    fn ocr_image_region_shifted(
        &self,
        image: &RgbImage,
        rect: (f64, f64, f64, f64),
        y_shift: f64,
        scaler: &CoordScaler,
    ) -> Result<String> {
        let (x, y, w, h) = rect;
        self.ocr_image_region(image, (x, y + y_shift, w, h), scaler)
    }

    /// Find artifact set key in OCR text (with multi-line fallback).
    ///
    /// Port of `findSetKeyInText()` from artifact_scanner.js
    fn find_set_key_in_text(&self, text: &str) -> Option<String> {
        if text.is_empty() {
            return None;
        }

        // Try full text first
        if let Some(key) = fuzzy_match_map(text, &self.mappings.artifact_set_map) {
            return Some(key);
        }

        // Try each line
        for line in text.split('\n') {
            let line = line.trim();
            if line.len() < 2 {
                continue;
            }
            if let Some(key) = fuzzy_match_map(line, &self.mappings.artifact_set_map) {
                return Some(key);
            }
        }

        None
    }

    /// Detect elixir crafted status from OCR.
    fn detect_elixir_crafted(
        &self,
        image: &RgbImage,
        scaler: &CoordScaler,
    ) -> Result<bool> {
        let text = self.ocr_image_region(image, self.ocr_regions.elixir, scaler)?;
        // "祝圣" = elixir
        Ok(text.contains("\u{795D}\u{5723}"))
    }

    /// Parse equipped character from equip text.
    fn parse_equip_location(&self, text: &str) -> String {
        if text.contains("\u{5DF2}\u{88C5}\u{5907}") {
            // "已装备"
            let char_name = text
                .replace("\u{5DF2}\u{88C5}\u{5907}", "")
                .replace([':', '\u{FF1A}', ' '], "")
                .trim()
                .to_string();
            if !char_name.is_empty() {
                return fuzzy_match_map(&char_name, &self.mappings.character_name_map)
                    .unwrap_or_default();
            }
        }
        String::new()
    }

    /// Scan a single artifact from a captured game image.
    ///
    /// Port of `scanSingleArtifact()` from GOODScanner/lib/artifact_scanner.js
    fn scan_single_artifact(
        &self,
        image: &RgbImage,
        scaler: &CoordScaler,
    ) -> Result<ArtifactScanResult> {
        // 0. Detect rarity — stop on 3-star or below
        let rarity = pixel_utils::detect_artifact_rarity(image, scaler);
        if rarity <= 3 {
            info!("[artifact] detected {}* item, stopping", rarity);
            return Ok(ArtifactScanResult::Stop);
        }

        // 1. Part name → slot key
        let part_text = self.ocr_image_region(image, self.ocr_regions.part_name, scaler)?;
        let slot_key = stat_parser::match_slot_key(&part_text);

        let slot_key = match slot_key {
            Some(k) => k.to_string(),
            None => {
                // 4-star with unrecognizable slot = possibly elixir essence, skip
                if rarity == 4 {
                    info!("[artifact] 4* unrecognizable slot (possibly elixir essence), skipping");
                    return Ok(ArtifactScanResult::Skip);
                }
                if self.config.continue_on_failure {
                    warn!("[artifact] cannot identify slot: \u{300C}{}\u{300D}, skipping", part_text);
                    return Ok(ArtifactScanResult::Skip);
                }
                bail!("Cannot identify artifact slot: \u{300C}{}\u{300D}", part_text);
            }
        };

        // 2. Main stat
        let main_stat_text = self.ocr_image_region(image, self.ocr_regions.main_stat, scaler)?;
        let main_stat_key = if slot_key == "flower" {
            Some("hp".to_string())
        } else if slot_key == "plume" {
            Some("atk".to_string())
        } else {
            stat_parser::parse_stat_from_text(&main_stat_text).map(|s| s.key)
        };

        let main_stat_key = match main_stat_key {
            Some(k) => k,
            None => {
                if self.config.continue_on_failure {
                    warn!("[artifact] cannot identify main stat: \u{300C}{}\u{300D}, skipping", main_stat_text);
                    return Ok(ArtifactScanResult::Skip);
                }
                bail!("Cannot identify main stat: \u{300C}{}\u{300D}", main_stat_text);
            }
        };

        // 3. Detect elixir crafted
        let elixir_crafted = self.detect_elixir_crafted(image, scaler)?;
        let y_shift = if elixir_crafted { ELIXIR_SHIFT } else { 0.0 };

        // 4. Level
        let level_text = self.ocr_image_region_shifted(image, self.ocr_regions.level, y_shift, scaler)?;
        let level = {
            let re = Regex::new(r"\+?\s*(\d+)").unwrap();
            re.captures(&level_text)
                .and_then(|c| c[1].parse::<i32>().ok())
                .unwrap_or(0)
        };

        // 5. Substats
        let subs_text = self.ocr_image_region_shifted(
            image,
            self.ocr_regions.substats,
            y_shift,
            scaler,
        )?;
        let mut substats: Vec<GoodSubStat> = Vec::new();
        let mut unactivated_substats: Vec<GoodSubStat> = Vec::new();

        if !subs_text.is_empty() {
            // Cut at "2件套" marker if present
            let subs_text = if let Some(idx) = subs_text.find("2\u{4EF6}\u{5957}") {
                &subs_text[..idx]
            } else {
                &subs_text
            };

            for line in subs_text.split('\n') {
                let line = line.trim();
                if line.len() < 2 {
                    continue;
                }
                if let Some(parsed) = stat_parser::parse_stat_from_text(line) {
                    let sub = GoodSubStat {
                        key: parsed.key,
                        value: parsed.value,
                    };
                    if parsed.inactive {
                        unactivated_substats.push(sub);
                    } else {
                        substats.push(sub);
                    }
                }
            }
        }

        // 6. Set name — position adjusted for number of substats
        let stat_count = (substats.len() + unactivated_substats.len()).clamp(1, 4);
        if stat_count < 4 && rarity == 5 {
            warn!("[artifact] 5* only identified {} substats", stat_count);
        }
        let missing_stats = 4 - stat_count as i32;
        let set_y = self.ocr_regions.set_name_base_y + y_shift - (missing_stats as f64 * 40.0);
        let set_name_text = self.ocr_image_region(
            image,
            (self.ocr_regions.set_name_x, set_y, self.ocr_regions.set_name_w, self.ocr_regions.set_name_h),
            scaler,
        )?;

        let set_key = self.find_set_key_in_text(&set_name_text);
        let set_key = match set_key {
            Some(k) => k,
            None => {
                let stat_keys: Vec<String> = substats
                    .iter()
                    .map(|s| s.key.clone())
                    .chain(unactivated_substats.iter().map(|s| format!("{}(inactive)", s.key)))
                    .collect();
                warn!(
                    "[artifact] cannot identify set: setY={} stats=[{}] text=\u{300C}{}\u{300D}",
                    set_y,
                    stat_keys.join(", "),
                    set_name_text
                );
                if self.config.continue_on_failure {
                    return Ok(ArtifactScanResult::Skip);
                }
                bail!(
                    "Cannot identify artifact set (substats={}): \u{300C}{}\u{300D}",
                    stat_count,
                    set_name_text
                );
            }
        };

        // 8. Equipped character
        let equip_text = self.ocr_image_region(image, self.ocr_regions.equip, scaler)?;
        let location = self.parse_equip_location(&equip_text);

        // 9. Lock
        let lock = pixel_utils::detect_artifact_lock(image, scaler, y_shift);

        // 10. Astral mark
        let astral_mark = pixel_utils::detect_artifact_astral_mark(image, scaler, y_shift);

        Ok(ArtifactScanResult::Artifact(GoodArtifact {
            set_key,
            slot_key,
            level,
            rarity,
            main_stat_key,
            substats,
            location,
            lock,
            astral_mark,
            elixir_crafted,
            unactivated_substats,
        }))
    }

    /// Generate a fingerprint for row-level deduplication.
    fn artifact_fingerprint(artifact: &GoodArtifact) -> String {
        let subs: Vec<String> = artifact
            .substats
            .iter()
            .map(|s| format!("{}:{}", s.key, s.value))
            .collect();
        format!(
            "{}|{}|{}|{}|{}|{}",
            artifact.set_key,
            artifact.slot_key,
            artifact.level,
            artifact.main_stat_key,
            artifact.rarity,
            subs.join(";")
        )
    }

    /// Scan all artifacts from the backpack.
    ///
    /// Uses `BackpackScanner` for grid traversal with panel-load detection
    /// and adaptive scrolling. The controller is passed in to avoid borrow
    /// conflicts between BackpackScanner and the scan callback.
    ///
    /// If `start_at > 0`, skips directly to that item index.
    pub fn scan(
        &self,
        ctrl: &mut GenshinGameController,
        skip_open_backpack: bool,
        start_at: usize,
    ) -> Result<Vec<GoodArtifact>> {
        info!("[artifact] starting scan...");
        let now = SystemTime::now();

        let mut bp = BackpackScanner::new(ctrl);

        if !skip_open_backpack {
            bp.open_backpack(self.config.open_delay);
        }
        bp.select_tab("artifact", self.config.delay_tab);

        let (_, total_count) = bp.read_item_count(self.ocr_model.as_ref())?;

        if total_count == 0 {
            warn!("[artifact] no artifacts in backpack");
            return Ok(Vec::new());
        }
        info!("[artifact] total: {}", total_count);

        let mut artifacts: Vec<GoodArtifact> = Vec::new();
        let mut fail_count = 0;

        // Row-level deduplication
        let mut seen_rows: Vec<String> = Vec::new();
        let mut current_row: Vec<String> = Vec::new();
        let mut pending_row: Vec<GoodArtifact> = Vec::new();

        let scan_config = BackpackScanConfig {
            delay_grid_item: self.config.delay_grid_item,
            delay_scroll: self.config.delay_scroll,
        };

        // Clone scaler so callback doesn't conflict with BackpackScanner's borrow
        let scaler = bp.scaler().clone();

        bp.scan_grid(
            total_count as usize,
            &scan_config,
            start_at,
            |event| {
                match event {
                    GridEvent::PageScrolled => {
                        // Clear row cache on page scroll
                        seen_rows.clear();
                        current_row.clear();
                        pending_row.clear();
                        return ScanAction::Continue;
                    }
                    GridEvent::Item(_idx, image) => {
                        match self.scan_single_artifact(image, &scaler) {
                            Ok(ArtifactScanResult::Artifact(artifact)) => {
                                let fingerprint = Self::artifact_fingerprint(&artifact);
                                current_row.push(fingerprint);
                                if artifact.rarity >= self.config.min_rarity {
                                    pending_row.push(artifact);
                                    fail_count = 0;
                                }
                            }
                            Ok(ArtifactScanResult::Stop) => {
                                return ScanAction::Stop;
                            }
                            Ok(ArtifactScanResult::Skip) => {
                                current_row.push("skip".to_string());
                                fail_count = 0;
                            }
                            Err(e) => {
                                error!("[artifact] scan error: {}", e);
                                current_row.push("null".to_string());
                                if !self.config.continue_on_failure {
                                    return ScanAction::Stop;
                                }
                                fail_count += 1;
                            }
                        }

                        // Row full → check deduplication
                        if current_row.len() >= GRID_COLS {
                            let row_str = current_row.join(",");
                            let is_dup = seen_rows.iter().any(|s| s == &row_str);

                            if is_dup {
                                warn!("[artifact] detected duplicate row, skipping {} items", pending_row.len());
                            } else {
                                seen_rows.push(row_str);
                                for a in pending_row.drain(..) {
                                    if self.config.log_progress {
                                        info!(
                                            "[artifact] {} {} +{} {}* {}{}{}",
                                            a.set_key, a.slot_key, a.level, a.rarity,
                                            if a.location.is_empty() { "-" } else { &a.location },
                                            if a.lock { " locked" } else { "" },
                                            if a.elixir_crafted { " elixir" } else { "" },
                                        );
                                    }
                                    artifacts.push(a);
                                }
                                fail_count = 0;
                            }
                            current_row.clear();
                            pending_row.clear();
                        }

                        if fail_count >= 10 {
                            error!("[artifact] {} consecutive failures, stopping", fail_count);
                            return ScanAction::Stop;
                        }

                        ScanAction::Continue
                    }
                }
            },
        );

        // Flush partial final row
        if !current_row.is_empty() {
            let row_str = current_row.join(",");
            let is_dup = seen_rows.iter().any(|s| s == &row_str);
            if !is_dup {
                for a in pending_row.drain(..) {
                    if self.config.log_progress {
                        info!(
                            "[artifact] {} {} +{} {}* {}{}{}",
                            a.set_key, a.slot_key, a.level, a.rarity,
                            if a.location.is_empty() { "-" } else { &a.location },
                            if a.lock { " locked" } else { "" },
                            if a.elixir_crafted { " elixir" } else { "" },
                        );
                    }
                    artifacts.push(a);
                }
            }
        }

        // Post-processing: remove unleveled 4-star artifacts from 5-star-capable sets
        let before_count = artifacts.len();
        artifacts.retain(|a| {
            if a.rarity == 4 && a.level == 0 {
                if let Some(&max_rarity) = self.mappings.artifact_set_max_rarity.get(&a.set_key) {
                    if max_rarity >= 5 {
                        return false;
                    }
                }
            }
            true
        });
        if artifacts.len() < before_count {
            info!(
                "[artifact] filtered {} unleveled 4* low-value artifacts",
                before_count - artifacts.len()
            );
        }

        info!(
            "[artifact] complete, {} artifacts scanned (>={}*) in {:?}",
            artifacts.len(),
            self.config.min_rarity,
            now.elapsed().unwrap_or_default()
        );

        Ok(artifacts)
    }

    /// Debug scan a single artifact from a captured image.
    ///
    /// Returns detailed per-field OCR results including raw text, parsed values,
    /// and timing information. Used by the re-scan debug mode.
    pub fn debug_scan_single(
        &self,
        image: &RgbImage,
        scaler: &CoordScaler,
    ) -> DebugScanResult {
        use std::time::Instant;

        let total_start = Instant::now();
        let mut fields = Vec::new();

        // Rarity (pixel)
        let t = Instant::now();
        let rarity = pixel_utils::detect_artifact_rarity(image, scaler);
        fields.push(DebugOcrField {
            field_name: "rarity".into(),
            raw_text: String::new(),
            parsed_value: format!("{}*", rarity),
            region: (0.0, 0.0, 0.0, 0.0),
            duration_ms: t.elapsed().as_millis() as u64,
        });

        // Part name → slot key
        let t = Instant::now();
        let part_text = self.ocr_image_region(image, self.ocr_regions.part_name, scaler)
            .unwrap_or_default();
        let slot_key = stat_parser::match_slot_key(&part_text)
            .map(|s| s.to_string())
            .unwrap_or_default();
        fields.push(DebugOcrField {
            field_name: "slot".into(),
            raw_text: part_text,
            parsed_value: slot_key.clone(),
            region: self.ocr_regions.part_name,
            duration_ms: t.elapsed().as_millis() as u64,
        });

        // Main stat
        let t = Instant::now();
        let main_stat_text = self.ocr_image_region(image, self.ocr_regions.main_stat, scaler)
            .unwrap_or_default();
        let main_stat_key = if slot_key == "flower" {
            "hp".to_string()
        } else if slot_key == "plume" {
            "atk".to_string()
        } else {
            stat_parser::parse_stat_from_text(&main_stat_text)
                .map(|s| s.key)
                .unwrap_or_default()
        };
        fields.push(DebugOcrField {
            field_name: "mainStat".into(),
            raw_text: main_stat_text,
            parsed_value: main_stat_key.clone(),
            region: self.ocr_regions.main_stat,
            duration_ms: t.elapsed().as_millis() as u64,
        });

        // Elixir detection
        let t = Instant::now();
        let elixir_crafted = self.detect_elixir_crafted(image, scaler).unwrap_or(false);
        let y_shift = if elixir_crafted { ELIXIR_SHIFT } else { 0.0 };
        fields.push(DebugOcrField {
            field_name: "elixir".into(),
            raw_text: String::new(),
            parsed_value: format!("{}", elixir_crafted),
            region: self.ocr_regions.elixir,
            duration_ms: t.elapsed().as_millis() as u64,
        });

        // Level
        let t = Instant::now();
        let level_text = self.ocr_image_region_shifted(image, self.ocr_regions.level, y_shift, scaler)
            .unwrap_or_default();
        let level = {
            let re = Regex::new(r"\+?\s*(\d+)").unwrap();
            re.captures(&level_text)
                .and_then(|c| c[1].parse::<i32>().ok())
                .unwrap_or(0)
        };
        fields.push(DebugOcrField {
            field_name: "level".into(),
            raw_text: level_text,
            parsed_value: format!("+{}", level),
            region: self.ocr_regions.level,
            duration_ms: t.elapsed().as_millis() as u64,
        });

        // Substats
        let t = Instant::now();
        let subs_text = self.ocr_image_region_shifted(
            image, self.ocr_regions.substats, y_shift, scaler,
        ).unwrap_or_default();
        let mut substats: Vec<GoodSubStat> = Vec::new();
        let mut unactivated_substats: Vec<GoodSubStat> = Vec::new();
        if !subs_text.is_empty() {
            let subs_cut = if let Some(idx) = subs_text.find("2\u{4EF6}\u{5957}") {
                &subs_text[..idx]
            } else {
                &subs_text
            };
            for line in subs_cut.split('\n') {
                let line = line.trim();
                if line.len() < 2 { continue; }
                if let Some(parsed) = stat_parser::parse_stat_from_text(line) {
                    let sub = GoodSubStat { key: parsed.key, value: parsed.value };
                    if parsed.inactive {
                        unactivated_substats.push(sub);
                    } else {
                        substats.push(sub);
                    }
                }
            }
        }
        let subs_summary: Vec<String> = substats.iter()
            .map(|s| format!("{}={}", s.key, s.value))
            .chain(unactivated_substats.iter().map(|s| format!("{}={}(inactive)", s.key, s.value)))
            .collect();
        fields.push(DebugOcrField {
            field_name: "substats".into(),
            raw_text: subs_text.replace('\n', " | "),
            parsed_value: subs_summary.join(", "),
            region: self.ocr_regions.substats,
            duration_ms: t.elapsed().as_millis() as u64,
        });

        // Set name
        let t = Instant::now();
        let stat_count = (substats.len() + unactivated_substats.len()).clamp(1, 4);
        let missing_stats = 4 - stat_count as i32;
        let set_y = self.ocr_regions.set_name_base_y + y_shift - (missing_stats as f64 * 40.0);
        let set_rect = (self.ocr_regions.set_name_x, set_y, self.ocr_regions.set_name_w, self.ocr_regions.set_name_h);
        let set_name_text = self.ocr_image_region(image, set_rect, scaler)
            .unwrap_or_default();
        let set_key = self.find_set_key_in_text(&set_name_text).unwrap_or_default();
        fields.push(DebugOcrField {
            field_name: "setName".into(),
            raw_text: set_name_text,
            parsed_value: set_key.clone(),
            region: set_rect,
            duration_ms: t.elapsed().as_millis() as u64,
        });

        // Equip
        let t = Instant::now();
        let equip_text = self.ocr_image_region(image, self.ocr_regions.equip, scaler)
            .unwrap_or_default();
        let location = self.parse_equip_location(&equip_text);
        fields.push(DebugOcrField {
            field_name: "equip".into(),
            raw_text: equip_text,
            parsed_value: if location.is_empty() { "(none)".into() } else { location.clone() },
            region: self.ocr_regions.equip,
            duration_ms: t.elapsed().as_millis() as u64,
        });

        // Lock + astral mark (pixel)
        let t = Instant::now();
        let lock = pixel_utils::detect_artifact_lock(image, scaler, y_shift);
        let astral_mark = pixel_utils::detect_artifact_astral_mark(image, scaler, y_shift);
        fields.push(DebugOcrField {
            field_name: "pixel_detect".into(),
            raw_text: String::new(),
            parsed_value: format!("lock={} astral={}", lock, astral_mark),
            region: (0.0, 0.0, 0.0, 0.0),
            duration_ms: t.elapsed().as_millis() as u64,
        });

        let artifact = GoodArtifact {
            set_key,
            slot_key,
            level,
            rarity,
            main_stat_key,
            substats,
            location,
            lock,
            astral_mark,
            elixir_crafted,
            unactivated_substats,
        };
        let parsed_json = serde_json::to_string_pretty(&artifact).unwrap_or_default();

        DebugScanResult {
            fields,
            total_duration_ms: total_start.elapsed().as_millis() as u64,
            parsed_json,
        }
    }
}

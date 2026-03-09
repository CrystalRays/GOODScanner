use yas::positioning::{Pos, Rect, Size};
use yas::window_info::FromWindowInfoRepository;

#[derive(Clone, Debug)]
pub struct ArtifactScannerWindowInfo {
    pub title_rect: Rect<f64>,
    pub main_stat_name_rect: Rect<f64>,
    pub main_stat_value_rect: Rect<f64>,
    pub sub_stat_1: Rect<f64>,
    pub sub_stat_2: Rect<f64>,
    pub sub_stat_3: Rect<f64>,
    pub sub_stat_4: Rect<f64>,
    pub level_rect: Rect<f64>,
    pub item_equip_rect: Rect<f64>,
    pub item_count_rect: Rect<f64>,
    pub star_pos: Pos<f64>,
    pub panel_rect: Rect<f64>,
    pub col: i32,
    pub row: i32,
    pub item_gap_size: Size<f64>,
    pub item_size: Size<f64>,
    pub scan_margin_pos: Pos<f64>,
    pub artifact_panel_offset: Size<f64>,
    pub lock_pos: Pos<f64>,
}

impl FromWindowInfoRepository for ArtifactScannerWindowInfo {
    fn from_window_info_repository(
        window_size: yas::positioning::Size<usize>,
        ui: yas::game_info::UI,
        platform: yas::game_info::Platform,
        repo: &yas::window_info::WindowInfoRepository,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            title_rect: repo.get_auto_scale("genshin_artifact_title_rect", window_size, ui, platform)
                .ok_or_else(|| anyhow::anyhow!("cannot find window info key \"genshin_artifact_title_rect\""))?,
            main_stat_name_rect: repo.get_auto_scale("genshin_artifact_main_stat_name_rect", window_size, ui, platform)
                .ok_or_else(|| anyhow::anyhow!("cannot find window info key \"genshin_artifact_main_stat_name_rect\""))?,
            main_stat_value_rect: repo.get_auto_scale("genshin_artifact_main_stat_value_rect", window_size, ui, platform)
                .ok_or_else(|| anyhow::anyhow!("cannot find window info key \"genshin_artifact_main_stat_value_rect\""))?,
            sub_stat_1: repo.get_auto_scale("genshin_artifact_sub_stat1_rect", window_size, ui, platform)
                .ok_or_else(|| anyhow::anyhow!("cannot find window info key \"genshin_artifact_sub_stat1_rect\""))?,
            sub_stat_2: repo.get_auto_scale("genshin_artifact_sub_stat2_rect", window_size, ui, platform)
                .ok_or_else(|| anyhow::anyhow!("cannot find window info key \"genshin_artifact_sub_stat2_rect\""))?,
            sub_stat_3: repo.get_auto_scale("genshin_artifact_sub_stat3_rect", window_size, ui, platform)
                .ok_or_else(|| anyhow::anyhow!("cannot find window info key \"genshin_artifact_sub_stat3_rect\""))?,
            sub_stat_4: repo.get_auto_scale("genshin_artifact_sub_stat4_rect", window_size, ui, platform)
                .ok_or_else(|| anyhow::anyhow!("cannot find window info key \"genshin_artifact_sub_stat4_rect\""))?,
            level_rect: repo.get_auto_scale("genshin_artifact_level_rect", window_size, ui, platform)
                .ok_or_else(|| anyhow::anyhow!("cannot find window info key \"genshin_artifact_level_rect\""))?,
            item_equip_rect: repo.get_auto_scale("genshin_artifact_item_equip_rect", window_size, ui, platform)
                .ok_or_else(|| anyhow::anyhow!("cannot find window info key \"genshin_artifact_item_equip_rect\""))?,
            item_count_rect: repo.get_auto_scale("genshin_artifact_item_count_rect", window_size, ui, platform)
                .ok_or_else(|| anyhow::anyhow!("cannot find window info key \"genshin_artifact_item_count_rect\""))?,
            star_pos: repo.get_auto_scale("genshin_artifact_star_pos", window_size, ui, platform)
                .ok_or_else(|| anyhow::anyhow!("cannot find window info key \"genshin_artifact_star_pos\""))?,
            panel_rect: repo.get_auto_scale("genshin_repository_panel_rect", window_size, ui, platform)
                .ok_or_else(|| anyhow::anyhow!("cannot find window info key \"genshin_repository_panel_rect\""))?,
            col: repo.get_auto_scale("genshin_repository_item_col", window_size, ui, platform)
                .ok_or_else(|| anyhow::anyhow!("cannot find window info key \"genshin_repository_item_col\""))?,
            row: repo.get_auto_scale("genshin_repository_item_row", window_size, ui, platform)
                .ok_or_else(|| anyhow::anyhow!("cannot find window info key \"genshin_repository_item_row\""))?,
            item_gap_size: repo.get_auto_scale("genshin_repository_item_gap_size", window_size, ui, platform)
                .ok_or_else(|| anyhow::anyhow!("cannot find window info key \"genshin_repository_item_gap_size\""))?,
            item_size: repo.get_auto_scale("genshin_repository_item_size", window_size, ui, platform)
                .ok_or_else(|| anyhow::anyhow!("cannot find window info key \"genshin_repository_item_size\""))?,
            scan_margin_pos: repo.get_auto_scale("genshin_repository_scan_margin_pos", window_size, ui, platform)
                .ok_or_else(|| anyhow::anyhow!("cannot find window info key \"genshin_repository_scan_margin_pos\""))?,
            artifact_panel_offset: repo.get_auto_scale("genshin_artifact_offset", window_size, ui, platform)
                .ok_or_else(|| anyhow::anyhow!("cannot find window info key \"genshin_artifact_offset\""))?,
            lock_pos: repo.get_auto_scale("genshin_repository_lock_pos", window_size, ui, platform)
                .ok_or_else(|| anyhow::anyhow!("cannot find window info key \"genshin_repository_lock_pos\""))?,
        })
    }
}

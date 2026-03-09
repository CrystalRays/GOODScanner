use yas::positioning::{Pos, Rect, Size};
use yas::window_info::FromWindowInfoRepository;

#[derive(Clone)]
pub struct GenshinRepositoryScanControllerWindowInfo {
    pub panel_rect: Rect<f64>,
    pub flag_pos: Pos<f64>,
    pub item_gap_size: Size<f64>,
    pub item_size: Size<f64>,
    pub scan_margin_pos: Pos<f64>,
    pub pool_rect: Rect<f64>,
    pub artifact_panel_offset: Size<f64>,
    pub genshin_repository_item_row: i32,
    pub genshin_repository_item_col: i32,
}

impl FromWindowInfoRepository for GenshinRepositoryScanControllerWindowInfo {
    fn from_window_info_repository(
        window_size: yas::positioning::Size<usize>,
        ui: yas::game_info::UI,
        platform: yas::game_info::Platform,
        repo: &yas::window_info::WindowInfoRepository,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            panel_rect: repo.get_auto_scale("genshin_repository_panel_rect", window_size, ui, platform)
                .ok_or_else(|| anyhow::anyhow!("cannot find window info key \"genshin_repository_panel_rect\""))?,
            flag_pos: repo.get_auto_scale("genshin_repository_flag_pos", window_size, ui, platform)
                .ok_or_else(|| anyhow::anyhow!("cannot find window info key \"genshin_repository_flag_pos\""))?,
            item_gap_size: repo.get_auto_scale("genshin_repository_item_gap_size", window_size, ui, platform)
                .ok_or_else(|| anyhow::anyhow!("cannot find window info key \"genshin_repository_item_gap_size\""))?,
            item_size: repo.get_auto_scale("genshin_repository_item_size", window_size, ui, platform)
                .ok_or_else(|| anyhow::anyhow!("cannot find window info key \"genshin_repository_item_size\""))?,
            scan_margin_pos: repo.get_auto_scale("genshin_repository_scan_margin_pos", window_size, ui, platform)
                .ok_or_else(|| anyhow::anyhow!("cannot find window info key \"genshin_repository_scan_margin_pos\""))?,
            pool_rect: repo.get_auto_scale("genshin_repository_pool_rect", window_size, ui, platform)
                .ok_or_else(|| anyhow::anyhow!("cannot find window info key \"genshin_repository_pool_rect\""))?,
            artifact_panel_offset: repo.get_auto_scale("genshin_artifact_offset", window_size, ui, platform)
                .ok_or_else(|| anyhow::anyhow!("cannot find window info key \"genshin_artifact_offset\""))?,
            genshin_repository_item_row: repo.get_auto_scale("genshin_repository_item_row", window_size, ui, platform)
                .ok_or_else(|| anyhow::anyhow!("cannot find window info key \"genshin_repository_item_row\""))?,
            genshin_repository_item_col: repo.get_auto_scale("genshin_repository_item_col", window_size, ui, platform)
                .ok_or_else(|| anyhow::anyhow!("cannot find window info key \"genshin_repository_item_col\""))?,
        })
    }
}

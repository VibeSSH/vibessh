use crate::errors::AppResult;
use crate::models::AppInfo;
use crate::state::AppState;

pub fn get_app_info(state: &AppState) -> AppResult<AppInfo> {
    Ok(AppInfo {
        name: state.app_name.clone(),
        version: state.app_version.clone(),
    })
}

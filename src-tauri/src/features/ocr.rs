use crate::{
    app::AppState,
    error::FlickError,
    models::{OcrRequest, OcrResponse},
    services::OcrService,
};

pub fn run(state: &AppState, request: OcrRequest) -> Result<OcrResponse, FlickError> {
    run_with_service(state.ocr_service.as_ref(), request)
}

pub fn run_with_service(
    service: &dyn OcrService,
    request: OcrRequest,
) -> Result<OcrResponse, FlickError> {
    service.run(request).map_err(Into::into)
}

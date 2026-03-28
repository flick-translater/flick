use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RendererBackend {
    NsPanel,
    CoreGraphics,
}

pub fn active_backend() -> RendererBackend {
    static BACKEND: OnceLock<RendererBackend> = OnceLock::new();
    *BACKEND.get_or_init(|| match std::env::var("FLICK_MACOS_CAPTURE_RENDERER") {
        Ok(value) if value.eq_ignore_ascii_case("nspanel") => RendererBackend::NsPanel,
        _ => RendererBackend::CoreGraphics,
    })
}

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::anyhow;
use once_cell::sync::Lazy;

use super::OcrService;
use crate::models::{OcrResponse, OcrTextBlock};

const CACHE_TTL_SECS: u64 = 300;
const MAX_CACHE_SIZE: usize = 100;

struct CacheEntry {
    text: String,
    timestamp: Instant,
}

static OCR_CACHE: Lazy<Arc<Mutex<HashMap<String, CacheEntry>>>> =
    Lazy::new(|| Arc::new(Mutex::new(HashMap::new())));

pub struct VisionOcrService;

impl OcrService for VisionOcrService {
    fn run_with_data(&self, image_data: &[u8]) -> anyhow::Result<OcrResponse> {
        let cache_key = generate_cache_key_from_data(image_data);

        if let Some(cached) = check_cache(&cache_key) {
            return Ok(OcrResponse {
                provider: "vision".into(),
                text: cached.clone(),
                blocks: vec![OcrTextBlock {
                    text: cached,
                    confidence: 1.0,
                }],
            });
        }

        let text = recognize_text_from_data(image_data)?;
        update_cache(cache_key, text.clone());

        Ok(OcrResponse {
            provider: "vision".into(),
            text: text.clone(),
            blocks: vec![OcrTextBlock {
                text,
                confidence: 1.0,
            }],
        })
    }
}

fn generate_cache_key_from_data(image_data: &[u8]) -> String {
    let mut hasher = DefaultHasher::new();
    image_data.hash(&mut hasher);
    format!("data:{}:{}", image_data.len(), hasher.finish())
}

fn check_cache(cache_key: &str) -> Option<String> {
    let cache = OCR_CACHE.lock().ok()?;
    let entry = cache.get(cache_key)?;

    if entry.timestamp.elapsed().as_secs() < CACHE_TTL_SECS {
        Some(entry.text.clone())
    } else {
        None
    }
}

fn update_cache(cache_key: String, text: String) {
    if let Ok(mut cache) = OCR_CACHE.lock() {
        if cache.len() >= MAX_CACHE_SIZE {
            let now = Instant::now();
            cache.retain(|_, entry| now.duration_since(entry.timestamp).as_secs() < CACHE_TTL_SECS);

            if cache.len() >= MAX_CACHE_SIZE {
                let oldest = cache
                    .iter()
                    .min_by_key(|(_, entry)| entry.timestamp)
                    .map(|(k, _)| k.clone());
                if let Some(key) = oldest {
                    cache.remove(&key);
                }
            }
        }

        cache.insert(
            cache_key,
            CacheEntry {
                text,
                timestamp: Instant::now(),
            },
        );
    }
}

#[cfg(target_os = "macos")]
mod vision_ffi {
    use std::ptr::null_mut;

    use anyhow::anyhow;
    use objc2::exception::catch;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use objc2_core_foundation::CGRect;
    use objc2_foundation::{NSArray, NSString};

    #[link(name = "ImageIO", kind = "framework")]
    unsafe extern "C" {
        fn CGImageSourceCreateWithData(
            data: *const std::ffi::c_void,
            options: *const std::ffi::c_void,
        ) -> *mut std::ffi::c_void;
        fn CGImageSourceCreateImageAtIndex(
            source: *mut std::ffi::c_void,
            index: usize,
            options: *const std::ffi::c_void,
        ) -> *mut std::ffi::c_void;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(cf: *mut std::ffi::c_void);
        fn CFDataCreate(
            allocator: *mut std::ffi::c_void,
            bytes: *const u8,
            length: usize,
        ) -> *mut std::ffi::c_void;
    }

    pub fn recognize_text_from_data(data: &[u8]) -> anyhow::Result<String> {
        catch(|| unsafe { recognize_text_impl(data) })
            .map_err(|e| anyhow!("Vision OCR failed with exception: {:?}", e))?
    }

    unsafe fn recognize_text_impl(data: &[u8]) -> anyhow::Result<String> {
        let data = data.to_vec();

        let cf_data = unsafe { CFDataCreate(std::ptr::null_mut(), data.as_ptr(), data.len()) };
        if cf_data.is_null() {
            return Err(anyhow!("Failed to create CFData"));
        }

        let image_source = unsafe {
            CGImageSourceCreateWithData(cf_data as *const std::ffi::c_void, std::ptr::null())
        };
        unsafe { CFRelease(cf_data) };

        if image_source.is_null() {
            return Err(anyhow!("Failed to create CGImageSource"));
        }

        let cg_image_ptr =
            unsafe { CGImageSourceCreateImageAtIndex(image_source, 0, std::ptr::null()) };
        unsafe { CFRelease(image_source) };

        if cg_image_ptr.is_null() {
            return Err(anyhow!("Failed to create CGImage"));
        }

        let handler_class = class!(VNImageRequestHandler);
        let handler: *mut AnyObject = msg_send![handler_class, alloc];
        let empty_dict: *mut AnyObject = msg_send![class!(NSDictionary), dictionary];
        let handler: *mut AnyObject = msg_send![
            handler,
            initWithCGImage: cg_image_ptr as *mut std::ffi::c_void,
            options: empty_dict
        ];

        unsafe { CFRelease(cg_image_ptr) };

        if handler.is_null() {
            return Err(anyhow!("Failed to create VNImageRequestHandler"));
        }

        let request_class = class!(VNRecognizeTextRequest);
        let request: *mut AnyObject = msg_send![request_class, alloc];
        let request: *mut AnyObject = msg_send![request, init];

        if request.is_null() {
            return Err(anyhow!("Failed to create VNRecognizeTextRequest"));
        }

        let level: i64 = 0;
        let () = msg_send![request, setRecognitionLevel: level];

        let lang_zh_hans = NSString::from_str("zh-Hans");
        let lang_zh_hant = NSString::from_str("zh-Hant");
        let lang_en = NSString::from_str("en");
        let lang_ja = NSString::from_str("ja");
        let languages: Retained<NSArray<NSString>> =
            NSArray::from_retained_slice(&[lang_zh_hans, lang_zh_hant, lang_en, lang_ja]);
        let () = msg_send![request, setRecognitionLanguages: AsRef::<NSArray<NSString>>::as_ref(&languages)];

        let automatically_detects_language: bool = true;
        let () =
            msg_send![request, setAutomaticallyDetectsLanguage: automatically_detects_language];

        let correction: bool = true;
        let () = msg_send![request, setUsesLanguageCorrection: correction];

        let region = CGRect::new(
            objc2_core_foundation::CGPoint::new(0.0, 0.0),
            objc2_core_foundation::CGSize::new(1.0, 1.0),
        );
        let () = msg_send![request, setRegionOfInterest: region];

        let request_retained = unsafe { Retained::from_raw(request).unwrap() };
        let requests: Retained<NSArray<AnyObject>> =
            NSArray::from_retained_slice(&[request_retained]);
        let mut error: *mut AnyObject = null_mut();
        let success: bool = msg_send![
            handler,
            performRequests: AsRef::<NSArray<AnyObject>>::as_ref(&requests),
            error: &mut error
        ];

        if !success {
            return Ok(String::new());
        }

        let observations: *mut AnyObject = msg_send![request, results];
        if observations.is_null() {
            return Ok(String::new());
        }

        unsafe { extract_text(observations) }
    }
    unsafe fn extract_text(observations: *mut AnyObject) -> anyhow::Result<String> {
        let count: usize = msg_send![observations, count];
        if count == 0 {
            return Ok(String::new());
        }

        let mut texts = Vec::with_capacity(count);
        for i in 0..count {
            let obs: *mut AnyObject = msg_send![observations, objectAtIndex: i];
            let candidates: *mut AnyObject = msg_send![obs, topCandidates: 1usize];
            if !candidates.is_null() {
                let cand_count: usize = msg_send![candidates, count];
                if cand_count > 0 {
                    let candidate: *mut AnyObject = msg_send![candidates, objectAtIndex: 0];
                    let text: *mut AnyObject = msg_send![candidate, string];
                    if !text.is_null() {
                        let nsstr = unsafe { Retained::from_raw(text.cast::<NSString>()) };
                        if let Some(s) = nsstr {
                            let string = s.to_string();
                            if !string.is_empty() {
                                texts.push(string);
                            }
                        }
                    }
                }
            }
        }

        Ok(texts.join("\n"))
    }
}

#[cfg(target_os = "macos")]
fn recognize_text_from_data(data: &[u8]) -> anyhow::Result<String> {
    vision_ffi::recognize_text_from_data(data)
}

#[cfg(not(target_os = "macos"))]
fn recognize_text_from_data(_data: &[u8]) -> anyhow::Result<String> {
    Err(anyhow!("Vision OCR is only available on macOS"))
}

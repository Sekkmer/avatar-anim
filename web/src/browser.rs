use js_sys::{Array, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Blob, File, HtmlAnchorElement, Url};

pub async fn read_file(file: File) -> Result<(String, Vec<u8>), String> {
    let name = file.name();
    let buffer = JsFuture::from(file.array_buffer())
        .await
        .map_err(js_error)?;
    let array = Uint8Array::new(&buffer);
    let mut bytes = vec![0; array.length() as usize];
    array.copy_to(&mut bytes);
    Ok((name, bytes))
}

pub fn download(filename: &str, bytes: &[u8]) -> Result<(), String> {
    let array = Uint8Array::from(bytes);
    let parts = Array::new();
    parts.push(&array.buffer());
    let blob = Blob::new_with_u8_array_sequence(&parts).map_err(js_error)?;
    let url = Url::create_object_url_with_blob(&blob).map_err(js_error)?;

    let window = web_sys::window().ok_or("Browser window is unavailable")?;
    let document = window.document().ok_or("Browser document is unavailable")?;
    let anchor = document
        .create_element("a")
        .map_err(js_error)?
        .dyn_into::<HtmlAnchorElement>()
        .map_err(|_| "Could not create download link".to_owned())?;
    anchor.set_href(&url);
    anchor.set_download(filename);
    anchor
        .style()
        .set_property("display", "none")
        .map_err(js_error)?;
    let body = document.body().ok_or("Browser document has no body")?;
    body.append_child(&anchor).map_err(js_error)?;
    anchor.click();
    body.remove_child(&anchor).map_err(js_error)?;
    Url::revoke_object_url(&url).map_err(js_error)
}

fn js_error(value: JsValue) -> String {
    value
        .as_string()
        .unwrap_or_else(|| format!("Browser error: {value:?}"))
}

use url::Url;

pub(super) const HOST: &str = "generativelanguage.googleapis.com";
pub(super) const OPENAI_PATH: &str = "/v1beta/openai";
pub(super) const MODELS_PATH: &str = "/v1beta/models";

pub(super) fn openai_base(url: &Url) -> Option<Url> {
    if url.scheme() != "https"
        || url.host_str() != Some(HOST)
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    match url.path().trim_end_matches('/') {
        ""
        | "/v1beta"
        | MODELS_PATH
        | OPENAI_PATH
        | "/v1beta/openai/models"
        | "/v1beta/openai/chat/completions" => {
            let mut base = url.clone();
            base.set_path(OPENAI_PATH);
            Some(base)
        }
        _ => None,
    }
}

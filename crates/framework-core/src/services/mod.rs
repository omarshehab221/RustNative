//! Platform-independent service contracts.
//!
//! Components only ever see [`Services`], a portable registry of optional
//! `Arc<dyn Trait>` contracts — never a platform backend's concrete type —
//! which is what keeps `crate::component` free of any platform dependency.
//! See `memory` for the deterministic in-memory implementations used by
//! tests and previews.

mod memory;

pub use memory::{MemoryClipboard, MemoryStorage};

use std::error::Error;
use std::fmt;
use std::sync::Arc;

/// An error returned by a platform service implementation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceError {
    message: String,
}

impl ServiceError {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }
}

impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for ServiceError {}

/// A validated HTTP method. `Other` covers verbs this enum doesn't name yet
/// (e.g. `WebDAV` extensions) without falling back to an unvalidated `String`
/// for the common cases.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Method {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Head,
    Options,
    Other(String),
}

impl Method {
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
            Self::Patch => "PATCH",
            Self::Head => "HEAD",
            Self::Options => "OPTIONS",
            Self::Other(value) => value,
        }
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An outgoing HTTP request.
///
/// Encapsulated (private fields + a constructor/builder) rather than a
/// plain-field struct: unlike this crate's geometry/color value types, a
/// request has a meaningful "this came from `HttpRequest::get`, so its
/// method and body are consistent" shape worth protecting, and it is the
/// kind of type application code constructs but never needs to
/// destructure/pattern-match field-by-field (contrast with
/// `crate::reconcile::TreeNode`, which every backend does need to
/// destructure).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    method: Method,
    url: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl HttpRequest {
    pub fn get(url: impl Into<String>) -> Self {
        Self { method: Method::Get, url: url.into(), headers: Vec::new(), body: Vec::new() }
    }

    pub fn new(method: Method, url: impl Into<String>) -> Self {
        Self { method, url: url.into(), headers: Vec::new(), body: Vec::new() }
    }

    #[must_use]
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    #[must_use]
    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }

    #[must_use]
    pub const fn method(&self) -> &Method {
        &self.method
    }
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }
    #[must_use]
    pub fn headers(&self) -> &[(String, String)] {
        &self.headers
    }
    #[must_use]
    pub fn body_bytes(&self) -> &[u8] {
        &self.body
    }
}

/// A received HTTP response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl HttpResponse {
    #[must_use]
    pub const fn new(status: u16, headers: Vec<(String, String)>, body: Vec<u8>) -> Self {
        Self { status, headers, body }
    }

    #[must_use]
    pub const fn status(&self) -> u16 {
        self.status
    }
    #[must_use]
    pub fn headers(&self) -> &[(String, String)] {
        &self.headers
    }
    #[must_use]
    pub fn body_bytes(&self) -> &[u8] {
        &self.body
    }
}

/// Executes a single HTTP request.
///
/// `#[async_trait::async_trait]` lets this stay a plain `async fn` in the
/// trait and in every impl below; the macro desugars each into a
/// `fn(...) -> Pin<Box<dyn Future<...> + Send>>` so the trait remains
/// `dyn`-compatible for `Arc<dyn HttpService>` (see [`Services`] below) — no
/// hand-written `Box::pin(async move { ... })` at every call site. This is
/// the one place this crate's service layer takes a dependency on the
/// `async-trait` macro's desugaring choice; see the module-level "why not
/// `ServiceFuture<T>`" note below for the alternative that was considered
/// and rejected.
///
/// (`trait_variant::make` was tried first here and reverted: it desugars
/// `async fn` into `-> impl Future<...> + Send` instead of a boxed future,
/// which is *not* `dyn`-compatible — exactly wrong for a trait this crate
/// needs to store as `Arc<dyn _>`.)
#[async_trait::async_trait]
pub trait HttpService: Send + Sync {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, ServiceError>;
}

/// Persistent key-value storage.
#[async_trait::async_trait]
pub trait StorageService: Send + Sync {
    async fn get(&self, key: String) -> Result<Option<Vec<u8>>, ServiceError>;
    async fn set(&self, key: String, value: Vec<u8>) -> Result<(), ServiceError>;
    async fn remove(&self, key: String) -> Result<(), ServiceError>;
}

/// The system clipboard, restricted to plain text.
#[async_trait::async_trait]
pub trait ClipboardService: Send + Sync {
    async fn read_text(&self) -> Result<Option<String>, ServiceError>;
    async fn write_text(&self, value: String) -> Result<(), ServiceError>;
}

/// Which native file-dialog affordance to present.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileDialogKind {
    /// A dialog for choosing one existing file to open.
    OpenFile,
    /// A dialog for choosing a destination path to save to.
    SaveFile,
    /// A dialog for choosing a folder.
    PickFolder,
}

/// Parameters for one native file-dialog invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDialogRequest {
    /// Which dialog affordance to present.
    pub kind: FileDialogKind,
    /// An optional window title override.
    pub title: Option<String>,
    /// Named filter groups, each a display label paired with the file
    /// extensions it matches (e.g. `("Images", vec!["png", "jpg"])`).
    pub filters: Vec<(String, Vec<String>)>,
}

/// A native open/save/pick-folder file dialog.
#[async_trait::async_trait]
pub trait FileDialogService: Send + Sync {
    async fn show(&self, request: FileDialogRequest) -> Result<Option<String>, ServiceError>;
}

/// Shell-level integration: launching URLs and posting system
/// notifications.
#[async_trait::async_trait]
pub trait SystemService: Send + Sync {
    async fn open_url(&self, url: String) -> Result<(), ServiceError>;
    async fn notify(&self, title: String, body: String) -> Result<(), ServiceError>;
}

/// Application-owned platform services. Components only receive this portable
/// contract and therefore never need to import a platform backend's API.
///
/// # Why not a `ServiceFuture<T>` type alias
///
/// An earlier version of this module additionally exported
/// `pub type ServiceFuture<T> = Pin<Box<dyn Future<Output = Result<T,
/// ServiceError>> + Send>>`, intended as a hand-written alternative to
/// `#[async_trait::async_trait]`'s desugaring. It was never actually used as
/// a trait signature anywhere — every trait above uses `async fn` through
/// the macro instead — so it was two public async abstractions for the same
/// concept with no call site choosing between them (standards audit P2.26).
/// Removed rather than adopted, since the macro form is what every impl
/// (including `framework-windows`'s) already commits to.
#[derive(Clone, Default)]
pub struct Services {
    http: Option<Arc<dyn HttpService>>,
    storage: Option<Arc<dyn StorageService>>,
    clipboard: Option<Arc<dyn ClipboardService>>,
    file_dialogs: Option<Arc<dyn FileDialogService>>,
    system: Option<Arc<dyn SystemService>>,
}

impl fmt::Debug for Services {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Services")
            .field("http", &self.http.is_some())
            .field("storage", &self.storage.is_some())
            .field("clipboard", &self.clipboard.is_some())
            .field("file_dialogs", &self.file_dialogs.is_some())
            .field("system", &self.system.is_some())
            .finish()
    }
}

impl Services {
    #[must_use]
    pub fn with_http(mut self, service: Arc<dyn HttpService>) -> Self {
        self.http = Some(service);
        self
    }
    #[must_use]
    pub fn with_storage(mut self, service: Arc<dyn StorageService>) -> Self {
        self.storage = Some(service);
        self
    }
    #[must_use]
    pub fn with_clipboard(mut self, service: Arc<dyn ClipboardService>) -> Self {
        self.clipboard = Some(service);
        self
    }
    #[must_use]
    pub fn with_file_dialogs(mut self, service: Arc<dyn FileDialogService>) -> Self {
        self.file_dialogs = Some(service);
        self
    }
    #[must_use]
    pub fn with_system(mut self, service: Arc<dyn SystemService>) -> Self {
        self.system = Some(service);
        self
    }
    #[must_use]
    pub fn http(&self) -> Option<&Arc<dyn HttpService>> {
        self.http.as_ref()
    }
    #[must_use]
    pub fn storage(&self) -> Option<&Arc<dyn StorageService>> {
        self.storage.as_ref()
    }
    #[must_use]
    pub fn clipboard(&self) -> Option<&Arc<dyn ClipboardService>> {
        self.clipboard.as_ref()
    }
    #[must_use]
    pub fn file_dialogs(&self) -> Option<&Arc<dyn FileDialogService>> {
        self.file_dialogs.as_ref()
    }
    #[must_use]
    pub fn system(&self) -> Option<&Arc<dyn SystemService>> {
        self.system.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_request_builder_round_trips() {
        let request = HttpRequest::get("https://example.test")
            .header("accept", "text/plain")
            .body(b"hi".to_vec());
        assert_eq!(request.method(), &Method::Get);
        assert_eq!(request.url(), "https://example.test");
        assert_eq!(request.headers(), &[("accept".to_owned(), "text/plain".to_owned())]);
        assert_eq!(request.body_bytes(), b"hi");
    }
}

// SPDX-License-Identifier: MIT

pub(crate) mod actions;
pub(crate) mod camera_preview;
pub(crate) mod diagnostics;
pub(crate) mod doc;
mod document;
pub(crate) mod document_media;
pub(crate) mod docx;
mod dropped_content;
pub(crate) mod file_activity;
pub(crate) mod file_providers;
mod file_source;
pub(crate) mod image_conversion;
mod install_source;
pub(crate) mod integration_catalog;
mod item_identity;
pub(crate) mod jobs;
mod listeners;
mod mime_type;
pub(crate) mod model_preview;
mod native_fs;
mod navigation_history;
mod network_error;
pub(crate) mod odt;
mod operations;
pub(crate) mod package_manager;
mod path_match;
mod preview;
mod release_channel;
mod remote_download;
pub(crate) mod route_preview;
pub(crate) mod rtf;
mod search;
pub(crate) mod table;
mod transfer_action;

pub use actions::{
    ActionAvailability, ActionCatalog, ActionHandle, ActionLoadFailure, ActionProgram,
    ActionRegistry, ActionScript, ActionWriteRequest, MatchedAction,
};
pub(crate) use document::{
    DocumentLayout, DocumentListChildKind, DocumentMedia, DocumentSpan, DocumentSpanStyle,
    DocumentTableCellLayout, DocumentUnit, DocumentUnitKind, document_kind, has_web_scheme,
    layout_document, parse_document,
};
pub(crate) use dropped_content::{
    DroppedFile, MAX_DROPPED_FILE_BYTES, WebLink, has_link_extension, save_dropped_files,
    save_new_file,
};
pub use file_source::{
    DirectoryChange, DirectoryEvent, DirectoryRequest, DirectoryStamp, FileSource, LoadHandle,
    LocationValidationError, MetadataOutcome, MetadataRequest, MetadataUpdate, MountToken,
    RequestId, Responsiveness, StampAnswer, UriCredentials, WatchRequest, Watching,
    backend_unavailable_message, sanitize_uri_credentials, validate_uri_credentials,
};
pub(crate) use file_source::{
    INVALID_NAME_REASON, UNEXPECTED_SYSTEM_ERROR, describes_refused_credentials, error_detail,
    gio_backend_error_message, gio_error_detail, gio_error_message, io_error_detail,
    io_error_message, sanitize_failure_message, system_error_text,
};
pub use install_source::InstallSource;
pub(crate) use install_source::installed_executable;
pub use item_identity::{Attributes, Evidence, IdentityTraits, InodeNumbers, ItemIdentity, Proof};
pub use jobs::{
    ActionEventSink, ActionRunEvent, ActionRunRequest, ActionRunner, CancelHandle,
    InvocationSource, JobId, JobRequest, JobService, JobSnapshot, JobStatus, ScriptProgress,
    expand_command_arguments,
};
pub(crate) use listeners::ListenerGuard;
pub use mime_type::{EntryType, FOLDER_TYPE_NAME, OTHER_TYPE_NAME, entry_type};
pub use model_preview::{ModelEncoding, ModelFormat, ModelMesh, ModelPalette, ModelView};
pub(crate) use model_preview::{ModelRender, ModelRenderPurpose};
pub(crate) use native_fs::{
    is_hidden_name, link_target_kind, listed_kind, native_hidden_names, native_kind, symlink_kind,
};
pub(crate) use navigation_history::NavigationHistory;
pub use operations::{
    ArchiveFormat, CancelledOperation, CompressRequest, CreateDirectoryRequest, CreateFileRequest,
    DeleteRequest, ExtractRequest, Holder, KeptCause, KeptItem, LeftoverInside, MoveRecord,
    NameMode, OperationEvent, OperationProvider, OperationRequestId, PasswordFailure, PasteItem,
    PasteRequest, PlaceEntryRequest, PlacedSnapshot, RecoverRequest, RenameCause, RenameRecord,
    RenameRequest, RenamedItem, ReplacedOriginal, RestoreRequest, RestoreSource, RestoreTrashItem,
    ReviewedLeftover, ShownName, SkippedItem, SpecialKind, TransferConflict, TransferReport,
    TypedPath, UndoCopyRequest, UndoMergeRequest, UndoMoveItem, UndoMoveRequest, UndoRenameRequest,
    validate_basename,
};
pub(crate) use path_match::{PathMatcher, PathQuery};
pub use preview::{
    ArchiveDirectory, ArchiveFileEntry, ArchiveNode, ArchivePreviewTree, MediaPreviewSize,
    ModelPreviewStage, PdfTextLayer, Preview, PreviewContent, PreviewEvent, PreviewProvider,
    PreviewRequest, PreviewRequestId, SandboxedMedia, SecretString, archive_preview_tree,
};
pub(crate) use preview::{
    INCORRECT_ARCHIVE_PASSWORD, archive_preview_format, bytes_support_named_family, content_family,
    guess_content_type, has_plain_text_extension, is_extensionless_dotfile, is_image_path,
    is_media_path, is_model, is_non_executable_extensionless_dotfile, looks_like_text,
    model_format, normalize_preview_text, read_head_sample, read_supports_named_family,
    reconcile_with_sample, refine_reported_content_type, split_archive_name, supports_remote_video,
};
pub(crate) use release_channel::{BuildKind, Version};
pub(crate) use remote_download::{
    RemoteDownload, download_remote, remote_file_name, remote_file_url,
};
pub(crate) use search::{
    RESULT_LIMIT as SEARCH_RESULT_LIMIT, RenameScope, rebase_search_indexes,
    refresh_search_indexes_for_directory, refresh_search_indexes_for_rename,
};
pub(crate) use search::{
    RefusedFolders, SearchCoverage, SearchEvent, SearchExclusions, SearchHandle, SearchItem,
    SearchLimits, filter_name_matches, filter_query_allows_typos, fold_for_search, index_filter,
    index_folder_paths, index_paths, index_trees_with_exclusions,
};
pub(crate) use transfer_action::{
    CrossVolumeDropStrategy, DropActionInput, DropCommit, DropOverride, TransferKind,
    VolumeIdentity, VolumeRelation, drop_commit, transferable_drop_sources, volume_relation,
};

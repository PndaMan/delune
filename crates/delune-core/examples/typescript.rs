//! Writes TypeScript declarations for every API type, for the web UI:
//!
//! ```sh
//! cargo run -q -p delune-core --example typescript --features ts > web/src/lib/api.generated.ts
//! ```
//!
//! CI regenerates the file and fails when it differs, so the web UI's types can't
//! quietly fall behind the server's.

use std::collections::BTreeMap;

// Every API type is listed below, so importing them all is the point.
#[allow(clippy::wildcard_imports)]
use delune_core::api::*;
use ts_rs::{Config, TS, TypeVisitor};

/// Collects a type and everything it refers to, by name.
struct Collect<'a> {
    cfg: &'a Config,
    decls: BTreeMap<String, String>,
}

impl Collect<'_> {
    fn add<T: TS + 'static + ?Sized>(&mut self) {
        let name = T::ident(self.cfg);
        let named = name.starts_with(|c: char| c.is_ascii_uppercase())
            && !matches!(name.as_str(), "Array" | "Record" | "Partial");
        if !named || self.decls.contains_key(&name) {
            return;
        }
        // Wrappers and primitives can't be declared; they panic instead.
        let cfg = self.cfg;
        let Ok(decl) = std::panic::catch_unwind(move || T::decl(cfg)) else { return };
        self.decls.insert(name, decl);
        T::visit_dependencies(self);
    }
}

impl TypeVisitor for Collect<'_> {
    fn visit<T: TS + 'static + ?Sized>(&mut self) {
        self.add::<T>();
    }
}

fn main() {
    // Unix seconds and byte counts fit comfortably in a JavaScript number.
    let cfg = Config::new().with_large_int("number");
    std::panic::set_hook(Box::new(|_| {}));
    let mut collect = Collect { cfg: &cfg, decls: BTreeMap::new() };
    macro_rules! types {
        ($($t:ty),* $(,)?) => { $(collect.add::<$t>();)* };
    }
    types!(
        Health,
        HealthStatus,
        SoulseekStatus,
        PortMapping,
        PortMappingState,
        SoulseekState,
        ApiError,
        CandidateFile,
        Candidate,
        QualityTier,
        RequestedFile,
        DownloadJobRequest,
        JobStatus,
        ReviewState,
        ReviewTrack,
        ReviewReport,
        ImportResult,
        FileStatus,
        JobFile,
        DownloadJob,
        Permissions,
        RequestStatus,
        MusicRequest,
        NewRequest,
        RequestDecision,
        NotificationKind,
        Notification,
        Notifications,
        AuthMode,
        Theme,
        Accent,
        Appearance,
        Me,
        LoginRequest,
        Person,
        SessionInfo,
        People,
        Presence,
        SoulseekUser,
        SoulseekProfile,
        ShareFolder,
        ShareTree,
        ChatMessage,
        ConversationSummary,
        RoomSummary,
        ChatOverview,
        RoomPerson,
        RoomView,
        ChatUpdate,
        SharingSettings,
        SpeedSchedule,
        SoulseekStats,
        SharingStatus,
        UploadStatus,
        Upload,
        MinQuality,
        WishlistItem,
        WishlistRequest,
        AutomationSettings,
        Follow,
        WishlistUpdate,
        ResolvedLink,
        MusicBrainzMatch,
        MatchedBy,
        ResolvedTrack,
        LibraryMatch,
        LibraryState,
        LibraryTrack,
        SearchEvent,
    );
    println!("// Generated from delune-core's API types. Don't edit by hand; regenerate with");
    println!("// cargo run -q -p delune-core --example typescript --features ts > web/src/lib/api.generated.ts");
    for decl in collect.decls.values() {
        println!();
        println!("export {}", decl.trim_end_matches(';'));
    }
}

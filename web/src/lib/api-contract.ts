// Checks at compile time that the web UI's types match the server's, as generated in
// api.generated.ts. A mismatch here means one side changed without the other.

import type * as Server from "@/lib/api.generated"
import type * as Web from "@/lib/api"
import type * as Web1 from "@/lib/appearance"
import type * as Web2 from "@/lib/automation"
import type * as Web3 from "@/lib/chat"
import type * as Web4 from "@/lib/library"
import type * as Web5 from "@/lib/notifications"
import type * as Web6 from "@/lib/requests"
import type * as Web7 from "@/lib/sharing"
import type * as Web8 from "@/lib/wishlist"

type Same<A, B> = [A] extends [B] ? ([B] extends [A] ? true : false) : false
type Expect<T extends true> = T

export type Contract = [
  Expect<Same<Web.Candidate, Server.Candidate>>,
  Expect<Same<Web.CandidateFile, Server.CandidateFile>>,
  Expect<Same<Web.Codec, Server.Codec>>,
  Expect<Same<Web.DownloadJob, Server.DownloadJob>>,
  Expect<Same<Web.DownloadJobRequest, Server.DownloadJobRequest>>,
  Expect<Same<Web.EntityKind, Server.EntityKind>>,
  Expect<Same<Web.FileStatus, Server.FileStatus>>,
  Expect<Same<Web.Health, Server.Health>>,
  Expect<Same<Web.ImportResult, Server.ImportResult>>,
  Expect<Same<Web.JobFile, Server.JobFile>>,
  Expect<Same<Web.JobStatus, Server.JobStatus>>,
  Expect<Same<Web.Me, Server.Me>>,
  Expect<Same<Web.MusicBrainzMatch, Server.MusicBrainzMatch>>,
  Expect<Same<Web.People, Server.People>>,
  Expect<Same<Web.Permissions, Server.Permissions>>,
  Expect<Same<Web.Person, Server.Person>>,
  Expect<Same<Web.Provider, Server.Provider>>,
  Expect<Same<Web.Quality, Server.Quality>>,
  Expect<Same<Web.ResolvedLink, Server.ResolvedLink>>,
  Expect<Same<Web.ResolvedTrack, Server.ResolvedTrack>>,
  Expect<Same<Web.ReviewReport, Server.ReviewReport>>,
  Expect<Same<Web.ReviewState, Server.ReviewState>>,
  Expect<Same<Web.ReviewTrack, Server.ReviewTrack>>,
  Expect<Same<Web.SearchEvent, Server.SearchEvent>>,
  Expect<Same<Web.SessionInfo, Server.SessionInfo>>,
  Expect<Same<Web.ShareFolder, Server.ShareFolder>>,
  Expect<Same<Web.ShareTree, Server.ShareTree>>,
  Expect<Same<Web.SoulseekState, Server.SoulseekState>>,
  Expect<Same<Web.SoulseekStatus, Server.SoulseekStatus>>,
  Expect<Same<Web.SoulseekUser, Server.SoulseekUser>>,
  Expect<Same<Web1.Accent, Server.Accent>>,
  Expect<Same<Web1.Appearance, Server.Appearance>>,
  Expect<Same<Web1.Theme, Server.Theme>>,
  Expect<Same<Web2.AutomationSettings, Server.AutomationSettings>>,
  Expect<Same<Web2.Follow, Server.Follow>>,
  Expect<Same<Web3.ChatMessage, Server.ChatMessage>>,
  Expect<Same<Web3.ChatOverview, Server.ChatOverview>>,
  Expect<Same<Web3.ConversationSummary, Server.ConversationSummary>>,
  Expect<Same<Web3.RoomPerson, Server.RoomPerson>>,
  Expect<Same<Web3.RoomSummary, Server.RoomSummary>>,
  Expect<Same<Web3.RoomView, Server.RoomView>>,
  Expect<Same<Web4.LibraryMatch, Server.LibraryMatch>>,
  Expect<Same<Web4.LibraryTrack, Server.LibraryTrack>>,
  Expect<Same<Web5.Notification, Server.Notification>>,
  Expect<Same<Web5.NotificationKind, Server.NotificationKind>>,
  Expect<Same<Web5.Notifications, Server.Notifications>>,
  Expect<Same<Web6.MusicRequest, Server.MusicRequest>>,
  Expect<Same<Web6.NewRequest, Server.NewRequest>>,
  Expect<Same<Web6.RequestStatus, Server.RequestStatus>>,
  Expect<Same<Web7.SharingSettings, Server.SharingSettings>>,
  Expect<Same<Web7.SharingStatus, Server.SharingStatus>>,
  Expect<Same<Web7.SoulseekStats, Server.SoulseekStats>>,
  Expect<Same<Web7.SpeedSchedule, Server.SpeedSchedule>>,
  Expect<Same<Web7.Upload, Server.Upload>>,
  Expect<Same<Web8.MinQuality, Server.MinQuality>>,
  Expect<Same<Web8.WishlistItem, Server.WishlistItem>>,
]

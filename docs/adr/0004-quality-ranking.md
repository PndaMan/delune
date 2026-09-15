# 0004 — One ordered quality key; match before quality

- **Status:** Accepted
- **Date:** 2026-09-15

## Context

"Highest quality" has to mean the same thing everywhere: Soulseek results,
streaming sources, files already in Navidrome, and the "better copy available"
check. Soulseek peers report quality inconsistently, and a perfect-quality folder
of the wrong edition is worse than a slightly lower-quality right one.

## Decision

**Quality** reduces to one integer, `Quality::rank()`:

1. Lossless beats lossy.
2. Among lossless files: bit depth, then sample rate (24/192 > 24/96 > 24/48 >
   24/44.1 > 16/44.1).
3. Among lossy files: bitrate adjusted for codec efficiency (Opus and AAC get credit
   over MP3).
4. A reported value beats the same value assumed; unknown bitrates are estimated
   from size and duration.

**Candidates** are sorted by, in order:

1. Match confidence against the tracklist (titles, track numbers, durations, every
   edition in the release group).
2. Completeness (all tracks present, no extras).
3. `Quality::rank`, penalising folders with mixed quality.
4. Availability: free upload slot, queue length, measured speed, and how reliable the
   peer has been for us before.

After download, files are decoded and spectrally checked. A suspected transcode is
flagged in review and the next candidate is offered.

## Consequences

- One tested function to change if the ordering ever needs to.
- Users who prefer 16-bit (smaller files, same audible result) need a setting that
  caps the preferred tier; planned for the settings screen.
- The ranking can't be fooled by a peer claiming 24/96 on an MP3, because
  verification runs before anything reaches review.

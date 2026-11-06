# ADR-002: Sync through a shared folder, one entry per photo

## Problem

Likes and images must reach several computers without an account or a server, and two computers can change them at the same time.

## Decision

The user picks a folder that an existing tool syncs. Each photo has its own entries in it, and an unlike leaves a removal marker. Entries are written complete and never edited in place by a second writer.

## Rationale

File-level sync tools merge separate files without conflict. A marker stops another computer from bringing an unliked photo back. The folder needs no credentials in the app.

## Rejected alternatives

- One shared list file: concurrent edits become conflict copies.
- WebDAV, Git, or a private server: more credentials or infrastructure, and the user chose the folder.

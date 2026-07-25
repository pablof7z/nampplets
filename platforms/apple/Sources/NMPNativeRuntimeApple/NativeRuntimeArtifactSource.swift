import Foundation
import NMPNativeRuntime

// MARK: - Profile-scoped registered artifact bytes

/// Immutable bytes supplied to Rust's signed-manifest resolver.
///
/// The callback does not decide whether a URL, digest, redirect, size, or
/// response is acceptable. It reports only the bytes and source selected by
/// the Rust-owned request; Rust revalidates every fact before sealing them.
final class RegisteredArtifactSource: ArtifactSource, @unchecked Sendable {
    private struct Entry {
        let bytes: Data
        var references: Int
    }

    private struct Registration {
        let digests: [String]
    }

    private static let maximumRegisteredBlobs = 256
    private static let maximumRegisteredBytes = 32 * 1_024 * 1_024

    private let lock = NSLock()
    private var entries: [String: Entry] = [:]
    private var registrations: [UUID: Registration] = [:]
    private var totalBytes = 0

    func register(_ blobsByDigest: [String: Data]) throws -> UUID {
        guard blobsByDigest.count <= Self.maximumRegisteredBlobs else {
            throw RuntimeNappletOpenError.artifactSourceRefused(
                detail: "A registration may contain at most \(Self.maximumRegisteredBlobs) blobs"
            )
        }
        let lowercaseHex = CharacterSet(charactersIn: "0123456789abcdef")
        for (digest, bytes) in blobsByDigest {
            guard digest.utf8.count == 64,
                  digest.unicodeScalars.allSatisfy(lowercaseHex.contains),
                  !bytes.isEmpty
            else {
                throw RuntimeNappletOpenError.artifactSourceRefused(
                    detail: "Every registered blob needs a lowercase SHA-256 digest and bytes"
                )
            }
        }

        lock.lock()
        defer { lock.unlock() }

        var additionalBytes = 0
        for (digest, bytes) in blobsByDigest {
            if let existing = entries[digest] {
                guard existing.bytes == bytes else {
                    throw RuntimeNappletOpenError.artifactSourceRefused(
                        detail: "Conflicting bytes were registered for digest \(digest)"
                    )
                }
            } else {
                additionalBytes += bytes.count
            }
        }
        let additionalCount = blobsByDigest.keys.filter { entries[$0] == nil }.count
        guard entries.count + additionalCount <= Self.maximumRegisteredBlobs,
              totalBytes + additionalBytes <= Self.maximumRegisteredBytes
        else {
            throw RuntimeNappletOpenError.artifactSourceRefused(
                detail: "The profile artifact source reached its finite registration limit"
            )
        }

        let token = UUID()
        for (digest, bytes) in blobsByDigest {
            if var existing = entries[digest] {
                existing.references += 1
                entries[digest] = existing
            } else {
                entries[digest] = Entry(bytes: bytes, references: 1)
                totalBytes += bytes.count
            }
        }
        registrations[token] = Registration(digests: blobsByDigest.keys.sorted())
        return token
    }

    func unregister(_ token: UUID) {
        lock.lock()
        defer { lock.unlock() }
        guard let registration = registrations.removeValue(forKey: token) else {
            return
        }
        for digest in registration.digests {
            guard var entry = entries[digest] else { continue }
            entry.references -= 1
            if entry.references == 0 {
                totalBytes -= entry.bytes.count
                entries.removeValue(forKey: digest)
            } else {
                entries[digest] = entry
            }
        }
    }

    func fetch(request: ArtifactFetchRequest) -> ArtifactFetchResponse {
        lock.lock()
        let bytes = entries[request.expectedSha256]?.bytes
        lock.unlock()
        guard let bytes else {
            return .refused(reason: "No bundled bytes match the requested digest")
        }
        guard let sourceURL = request.candidateUrls.first else {
            return .refused(reason: "The verified manifest has no candidate source")
        }
        return .body(sourceUrl: sourceURL, httpStatus: 200, bytes: bytes)
    }
}

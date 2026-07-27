import Foundation
import NMPNativeRuntime

// MARK: - Signed install, launch, and borrowed session plumbing

extension NativeRuntimeProfile {
    /// Verifies and installs one exact named build without granting or
    /// launching it. The returned opaque handle is bound to this profile.
    public func installSignedNamed(
        title: String,
        eventJSON: Data,
        author: String,
        dTag: String,
        blobsBySHA256: [String: Data]
    ) throws -> NativeRuntimeInstalledArtifact {
        lock.lock()
        let closed = isClosed
        lock.unlock()
        guard !closed else {
            throw RuntimeNappletOpenError.installRefused(
                detail: "The application runtime profile is closed"
            )
        }

        let registration = try source.register(blobsBySHA256)
        defer { source.unregister(registration) }
        let verification = controller.verifyArtifact(
            eventJson: eventJSON,
            coordinate: .named(author: author, dTag: dTag)
        )
        guard let artifact = verification.artifact else {
            let refusal = verification.refusal
            throw RuntimeNappletOpenError.artifactRefused(
                code: refusal?.code ?? "missing-artifact",
                detail: refusal?.detail ?? "No sealed artifact was returned"
            )
        }

        controller.install(artifact: artifact)
        guard let verifiedDTag = artifact.dTag() else {
            throw RuntimeNappletOpenError.installRefused(
                detail: "The verified named artifact has no dTag"
            )
        }
        let coordinate = NativeRuntimePermissionCoordinate(
            manifestAuthor: artifact.author(),
            dTag: verifiedDTag,
            aggregateHash: artifact.aggregateHash()
        )
        let installedReview = controller.permissionReview(
            coordinate: coordinate
        )
        guard installedReview.refusal == nil,
              installedReview.review?.coordinate == coordinate
        else {
            throw RuntimeNappletOpenError.installRefused(
                detail: installedReview.refusal?.detail
                    ?? "The installed exact build was not projected by Rust"
            )
        }
        return NativeRuntimeInstalledArtifact(
            title: title,
            ownerID: profileID,
            artifact: artifact,
            permissionCoordinate: coordinate
        )
    }

    /// Launches one already-installed exact build. Permission application is a
    /// separate Rust transaction and is never performed by this operation.
    public func launchInstalled(
        _ installed: NativeRuntimeInstalledArtifact
    ) throws -> NappletArtifact {
        guard installed.ownerID == profileID else {
            throw RuntimeNappletOpenError.installedArtifactProfileMismatch
        }
        lock.lock()
        let closed = isClosed
        lock.unlock()
        guard !closed else {
            throw RuntimeNappletOpenError.launchRefused(
                detail: "The application runtime profile is closed"
            )
        }

        let artifact = installed.artifact
        let coordinate = installed.permissionCoordinate
        let priorProjection = pullSnapshotProjection()
        let priorSnapshot: RuntimeSnapshot
        switch priorProjection {
        case let .snapshot(snapshot):
            priorSnapshot = snapshot
        case let .refused(_, _, refusal):
            throw RuntimeNappletOpenError.launchRefused(
                detail: "\(refusal.code): \(refusal.detail)"
            )
        }
        let priorSessions = Set(priorSnapshot.sessions.map(\.id))
        controller.launch(artifact: artifact, profile: .legacy)

        let launchedProjection = pullSnapshotProjection()
        let launchedSnapshot: RuntimeSnapshot
        switch launchedProjection {
        case let .snapshot(snapshot):
            launchedSnapshot = snapshot
        case let .refused(_, _, refusal):
            throw RuntimeNappletOpenError.launchRefused(
                detail: "\(refusal.code): \(refusal.detail)"
            )
        }
        guard let launched = launchedSnapshot.sessions.first(where: {
            !priorSessions.contains($0.id)
                && $0.author == coordinate.manifestAuthor
                && $0.dTag == coordinate.dTag
                && $0.aggregateHash == coordinate.aggregateHash
                // Degraded counts as created. The runtime launches without a
                // required domain on purpose, so refusing here would turn
                // "started without `lists`" into "failed to start" and make
                // most real napplets unlaunchable.
                && ($0.state == "running" || $0.state == "running-degraded")
        }) else {
            let detail = launchedSnapshot.recentErrors.last?.detail
                ?? launchedSnapshot.boundaryRefusals.last?.detail
                ?? "No new running session was created"
            throw RuntimeNappletOpenError.launchRefused(detail: detail)
        }

        let runtime = RustRuntimeNappletSession(
            profile: self,
            sessionID: launched.id,
            maximumReadBytes: Self.maximumReadBytes
        )
        lock.lock()
        if isClosed {
            lock.unlock()
            controller.stop(sessionId: launched.id)
            throw RuntimeNappletOpenError.launchRefused(
                detail: "The application runtime profile closed during launch"
            )
        }
        sessions[launched.id] = WeakSession(runtime)
        lock.unlock()

        return NappletArtifact(
            title: installed.title,
            reader: runtime,
            runtimeSession: runtime,
            negotiatedDomains: launched.domains
        )
    }

    /// Compatibility helper retained for existing Apple package callers.
    /// Product launch flows must use install, atomic permission review, and
    /// launch as separate operations.
    public func openSignedNamed(
        title: String,
        eventJSON: Data,
        author: String,
        dTag: String,
        blobsBySHA256: [String: Data],
        grantDomains: [String]
    ) throws -> NappletArtifact {
        let installed = try installSignedNamed(
            title: title,
            eventJSON: eventJSON,
            author: author,
            dTag: dTag,
            blobsBySHA256: blobsBySHA256
        )
        for domain in grantDomains {
            controller.setGrant(
                artifact: installed.artifact,
                capability: domain,
                sensitivity: .ordinary,
                decision: .allowExactBuild
            )
        }
        return try launchInstalled(installed)
    }

    func readVerified(
        sessionID: UInt64,
        logicalPath: String,
        maximumBytes: UInt64
    ) -> VerifiedRead {
        controller.readVerified(
            sessionId: sessionID,
            logicalPath: logicalPath,
            maximumBytes: maximumBytes
        )
    }

    func mappedEnvelope(sessionID: UInt64, bytes: Data) {
        controller.mappedEnvelope(sessionId: sessionID, bytes: bytes)
    }

    func stopSession(_ sessionID: UInt64) {
        lock.lock()
        let shouldStop = !isClosed && sessions.removeValue(forKey: sessionID) != nil
        lock.unlock()
        if shouldStop {
            controller.stop(sessionId: sessionID)
        }
    }

    func crashSession(_ sessionID: UInt64, reason: String) {
        lock.lock()
        let shouldCrash = !isClosed && sessions[sessionID]?.value != nil
        lock.unlock()
        if shouldCrash {
            controller.crash(sessionId: sessionID, reason: reason)
        }
    }
}

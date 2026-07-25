import Foundation
import NMPNativeRuntime

// MARK: - Account registration, activation, and secure persistence

extension NativeRuntimeProfile {
    public func accountSnapshot() -> NativeRuntimeAccountUpdate {
        accountLock.lock()
        let update = controller.accountSnapshot()
        accountLock.unlock()
        return update
    }

    public func registerLocalAccount(
        secretKey: String
    ) -> NativeRuntimeAccountUpdate {
        accountLock.lock()
        let update = controller.registerLocalAccount(secretKey: secretKey)
        if
            update.accepted,
            let handle = update.handle,
            let accountVault
        {
            do {
                try accountVault.upsertLocalSigner(
                    publicKey: handle.publicKey,
                    secret: secretKey,
                    maximumAccounts: Self.maximumAccounts
                )
            } catch {
                accountPersistenceProblem =
                    accountPersistenceProblem ?? .registerFailed
            }
        }
        accountLock.unlock()
        return update
    }

    public func registerReadOnlyAccount(
        publicIdentity: String
    ) -> NativeRuntimeAccountUpdate {
        accountLock.lock()
        let update = controller.registerReadOnlyAccount(
            publicIdentity: publicIdentity
        )
        if
            update.accepted,
            let handle = update.handle,
            let accountVault
        {
            do {
                try accountVault.upsertReadOnly(
                    publicKey: handle.publicKey,
                    maximumAccounts: Self.maximumAccounts
                )
            } catch {
                accountPersistenceProblem =
                    accountPersistenceProblem ?? .registerFailed
            }
        }
        accountLock.unlock()
        return update
    }

    public func activateLocalAccount(
        handle: NativeRuntimeAccountHandle
    ) -> NativeRuntimeAccountUpdate {
        accountLock.lock()
        let update = controller.activateLocalAccount(handle: handle)
        if update.accepted, let accountVault {
            do {
                try accountVault.setActive(
                    publicKey: update.snapshot?.activePublicKey,
                    maximumAccounts: Self.maximumAccounts
                )
            } catch {
                accountPersistenceProblem =
                    accountPersistenceProblem ?? .activationFailed
            }
        }
        accountLock.unlock()
        return update
    }

    public func logoutLocalAccount() -> NativeRuntimeAccountUpdate {
        accountLock.lock()
        let update = controller.logoutLocalAccount()
        if update.accepted, let accountVault {
            do {
                try accountVault.setActive(
                    publicKey: nil,
                    maximumAccounts: Self.maximumAccounts
                )
            } catch {
                accountPersistenceProblem =
                    accountPersistenceProblem ?? .logoutFailed
            }
        }
        accountLock.unlock()
        return update
    }

    public func removeLocalAccount(
        handle: NativeRuntimeAccountHandle
    ) -> NativeRuntimeAccountUpdate {
        accountLock.lock()
        let update = controller.removeLocalAccount(handle: handle)
        if update.accepted, let accountVault {
            do {
                try accountVault.remove(
                    publicKey: handle.publicKey,
                    maximumAccounts: Self.maximumAccounts
                )
            } catch {
                accountPersistenceProblem =
                    accountPersistenceProblem ?? .removalFailed
            }
        }
        accountLock.unlock()
        return update
    }

    public func accountPersistenceIssue()
        -> NativeRuntimeAccountPersistenceIssue?
    {
        accountLock.lock()
        let issue = accountPersistenceProblem
        accountLock.unlock()
        return issue
    }

    func restorePersistedAccounts() {
        guard let accountVault else {
            return
        }

        accountLock.lock()
        defer { accountLock.unlock() }
        let stored: NativeAccountVaultSnapshot
        do {
            stored = try accountVault.load(
                maximumAccounts: Self.maximumAccounts
            )
        } catch {
            accountPersistenceProblem = .restoreFailed
            return
        }

        var restoredHandles: [
            String: NativeRuntimeAccountHandle
        ] = [:]
        restoredHandles.reserveCapacity(stored.accounts.count)
        var restoreFailed = false
        for account in stored.accounts {
            let update: NativeRuntimeAccountUpdate
            switch account.material {
            case let .localSigner(secret):
                update = controller.registerLocalAccount(
                    secretKey: secret
                )
            case .readOnly:
                update = controller.registerReadOnlyAccount(
                    publicIdentity: account.publicKey
                )
            }
            guard
                update.accepted,
                let handle = update.handle,
                handle.publicKey == account.publicKey
            else {
                if let unexpectedHandle = update.handle {
                    _ = controller.removeLocalAccount(
                        handle: unexpectedHandle
                    )
                }
                restoreFailed = true
                continue
            }
            restoredHandles[account.publicKey] = handle
        }

        if let activePublicKey = stored.activePublicKey {
            guard let activeHandle = restoredHandles[activePublicKey] else {
                accountPersistenceProblem = .restoreFailed
                let revision = controller.snapshot().revision
                lastActivityRevision = revision
                lastLibraryRevision = revision
                return
            }
            let activation = controller.activateLocalAccount(
                handle: activeHandle
            )
            if
                !activation.accepted
                    || activation.snapshot?.activePublicKey != activePublicKey
            {
                restoreFailed = true
            }
        }
        accountPersistenceProblem = restoreFailed ? .restoreFailed : nil
        let revision = controller.snapshot().revision
        lastActivityRevision = revision
        lastLibraryRevision = revision
    }
}

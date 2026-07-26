import Foundation
import NMPNativeRuntime

final class RecordingRuntimeObserver: RuntimeObserver, @unchecked Sendable {
    private let condition = NSCondition()
    private var revision: UInt64?

    var latestRevision: UInt64? {
        condition.lock()
        defer { condition.unlock() }
        return revision
    }

    func update(frame: RuntimeObservationFrame) {
        let deliveredRevision: UInt64?
        switch frame.snapshot {
        case let .snapshot(snapshot):
            deliveredRevision = snapshot.revision
        case let .refused(revision, _, _):
            deliveredRevision = revision
        @unknown default:
            deliveredRevision = nil
        }
        condition.lock()
        revision = deliveredRevision
        condition.broadcast()
        condition.unlock()
    }

    func waitForInitialFrame(timeout: TimeInterval) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        condition.lock()
        defer { condition.unlock() }
        while revision == nil {
            guard condition.wait(until: deadline) else {
                return false
            }
        }
        return true
    }
}

final class ResponseRuntimeObserver: RuntimeObserver, @unchecked Sendable {
    private let condition = NSCondition()
    private var responses: [String] = []

    func update(frame: RuntimeObservationFrame) {
        let delivered = frame.events.compactMap(\.responseJson)
        guard !delivered.isEmpty else { return }
        condition.lock()
        responses.append(contentsOf: delivered)
        condition.broadcast()
        condition.unlock()
    }

    func waitForResponse(
        type: String,
        id: String?,
        timeout: TimeInterval
    ) -> [String: Any]? {
        let deadline = Date().addingTimeInterval(timeout)
        condition.lock()
        defer { condition.unlock() }
        while true {
            if let response = responses.compactMap(decode).first(where: {
                $0["type"] as? String == type
                    && (id == nil || $0["id"] as? String == id)
            }) {
                return response
            }
            guard condition.wait(until: deadline) else {
                return nil
            }
        }
    }

    private func decode(_ raw: String) -> [String: Any]? {
        guard
            let data = raw.data(using: .utf8),
            let value = try? JSONSerialization.jsonObject(with: data)
        else {
            return nil
        }
        return value as? [String: Any]
    }
}

final class TeardownRuntimeObserver: RuntimeObserver, @unchecked Sendable {
    private enum State: String {
        case waitingForCallback
        case callbackBlocked
        case cancelled
        case callbackReleased
        case terminal
        case callbackTimedOut
    }

    private let condition = NSCondition()
    private var state = State.waitingForCallback
    private var callbackReleased = false
    private var cancelled = false
    private var ignoredFrames = 0

    var lastState: String {
        condition.lock()
        defer { condition.unlock() }
        return "callback lifecycle state: \(state.rawValue)"
    }

    var ignoredFramesAfterCancellation: Int {
        condition.lock()
        defer { condition.unlock() }
        return ignoredFrames
    }

    func update(frame _: RuntimeObservationFrame) {
        let deadline = Date().addingTimeInterval(2)
        condition.lock()
        state = .callbackBlocked
        condition.broadcast()
        while !callbackReleased {
            guard condition.wait(until: deadline) else {
                state = .callbackTimedOut
                condition.broadcast()
                condition.unlock()
                return
            }
        }
        state = .callbackReleased
        if cancelled {
            ignoredFrames += 1
        }
        state = .terminal
        condition.broadcast()
        condition.unlock()
    }

    func waitForCallbackEntry(timeout: TimeInterval) -> Bool {
        waitForState(timeout: timeout) {
            $0 != .waitingForCallback
        }
    }

    func cancel() {
        condition.lock()
        cancelled = true
        state = .cancelled
        condition.broadcast()
        condition.unlock()
    }

    func releaseCallback() {
        condition.lock()
        callbackReleased = true
        condition.broadcast()
        condition.unlock()
    }

    func waitForTerminalState(timeout: TimeInterval) -> Bool {
        waitForState(timeout: timeout) {
            $0 == .terminal || $0 == .callbackTimedOut
        } && stateSnapshot() == .terminal
    }

    private func waitForState(
        timeout: TimeInterval,
        predicate: (State) -> Bool
    ) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        condition.lock()
        defer { condition.unlock() }
        while !predicate(state) {
            guard condition.wait(until: deadline) else {
                return false
            }
        }
        return true
    }

    private func stateSnapshot() -> State {
        condition.lock()
        defer { condition.unlock() }
        return state
    }
}

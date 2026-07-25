import Foundation

public enum RuntimeWorkbenchLibraryAdmissionRefusal:
    Error,
    LocalizedError,
    Equatable
{
    case subscriberCapacity(maximum: Int)

    public var errorDescription: String? {
        switch self {
        case .subscriberCapacity(let maximum):
            "The Workbench library subscriber limit of \(maximum) was reached."
        }
    }
}

enum RuntimeWorkbenchLibraryProjectionError:
    Error,
    LocalizedError
{
    case invalidExactBuild
    case invalidBuild
    case invalidWorkspace
    case invalidRefusal
    case invalidSnapshot

    var errorDescription: String? {
        switch self {
        case .invalidExactBuild:
            "The native projection contained an invalid exact-build identity."
        case .invalidBuild:
            "The native projection contained an invalid installed-build row."
        case .invalidWorkspace:
            "The native projection contained an invalid workspace row."
        case .invalidRefusal:
            "The native projection contained an invalid refusal."
        case .invalidSnapshot:
            "The native projection exceeded the Workbench snapshot contract."
        }
    }
}

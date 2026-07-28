import Nimble
import Quick
@testable import RuntimeWorkbenchFeature

/// Request generations are how a catalog response proves it is still the one
/// being waited for. Wrapping arithmetic would let an old response share the
/// current generation again after `UInt.max` and pass that check — so the
/// counter is checked, and running out is a terminal state rather than a
/// return to zero.
///
/// The limit is remote in ordinary use. That is exactly why it needs pinning:
/// nothing else will exercise it.
final class CatalogRequestGenerationSpec: QuickSpec {
    override class func spec() {
        describe("A catalog request generation lane") {
            context("given the lane is one generation from its maximum") {
                it("issues the last valid generation exactly once") {
                    var counter = CatalogRequestGenerationCounter(
                        lane: .feed,
                        current: UInt.max - 1
                    )

                    expect(counter.issue()).to(equal(UInt.max))
                    expect(counter.exhaustion).to(beNil())
                    expect(counter.isCurrent(UInt.max)).to(beTrue())
                }

                it("refuses the next generation terminally and stays refused") {
                    var counter = CatalogRequestGenerationCounter(
                        lane: .feed,
                        current: UInt.max - 1
                    )
                    _ = counter.issue()

                    expect(counter.issue()).to(beNil())
                    expect(counter.exhaustion?.lane).to(equal(.feed))
                    expect(counter.exhaustion?.exhaustedGeneration)
                        .to(equal(UInt.max))

                    // Terminal means terminal: a later attempt does not
                    // recover, and does not restate the exhaustion at a
                    // different generation either.
                    expect(counter.issue()).to(beNil())
                    expect(counter.exhaustion?.exhaustedGeneration)
                        .to(equal(UInt.max))
                }

                it("never lets a stale generation become current again") {
                    var counter = CatalogRequestGenerationCounter(
                        lane: .transientOperation,
                        current: UInt.max - 1
                    )
                    let last = counter.issue()
                    _ = counter.issue()

                    // The response that was in flight when the lane ran out
                    // must not be admitted by aliasing back onto its own
                    // token.
                    expect(counter.isCurrent(last!)).to(beFalse())
                    expect(counter.isCurrent(0)).to(beFalse())
                    expect(counter.isCurrent(UInt.max)).to(beFalse())
                }
            }

            context("given ordinary use well below the maximum") {
                it("ignores a response carrying an earlier token") {
                    var counter = CatalogRequestGenerationCounter(lane: .feed)
                    let first = counter.issue()
                    let second = counter.issue()

                    expect(counter.isCurrent(first!)).to(beFalse())
                    expect(counter.isCurrent(second!)).to(beTrue())
                }

                it("issues strictly increasing, never-repeated generations") {
                    var counter = CatalogRequestGenerationCounter(lane: .feed)
                    var seen: Set<UInt> = []

                    for _ in 0..<64 {
                        let issued = counter.issue()
                        expect(issued).toNot(beNil())
                        expect(seen.contains(issued!)).to(beFalse())
                        seen.insert(issued!)
                    }
                }
            }

            context("given the two lanes run out independently") {
                it("attributes exhaustion to the lane that ran out") {
                    var feed = CatalogRequestGenerationCounter(
                        lane: .feed,
                        current: UInt.max
                    )
                    var operations = CatalogRequestGenerationCounter(
                        lane: .transientOperation,
                        current: UInt.max
                    )

                    _ = feed.issue()
                    _ = operations.issue()

                    expect(feed.exhaustion?.lane).to(equal(.feed))
                    expect(operations.exhaustion?.lane)
                        .to(equal(.transientOperation))
                    expect(feed.exhaustion?.technicalDetail)
                        .to(contain("feed"))
                    expect(operations.exhaustion?.technicalDetail)
                        .to(contain("transientOperation"))
                }

                /// Saturating instead of refusing would leave every request
                /// holding the same token, which is the aliasing this exists
                /// to prevent.
                it("does not saturate at the maximum") {
                    var counter = CatalogRequestGenerationCounter(
                        lane: .feed,
                        current: UInt.max
                    )

                    expect(counter.issue()).to(beNil())
                    expect(counter.current).to(equal(UInt.max))
                }
            }

            context("given exhaustion evidence is read") {
                it("keeps the exact generation alongside a bounded summary") {
                    let exhaustion = CatalogRequestGenerationExhaustion(
                        lane: .feed,
                        exhaustedGeneration: UInt.max
                    )

                    expect(exhaustion.exhaustedGeneration).to(equal(UInt.max))
                    expect(exhaustion.technicalDetail)
                        .to(contain("\(UInt.max)"))
                }
            }
        }
    }
}

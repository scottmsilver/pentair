import Foundation

private func makeSystem(poolSetpoint: Int = 80) -> PoolSystem {
    PoolSystem(
        pool: BodyState(
            on: true,
            active: true,
            temperature: 88,
            setpoint: poolSetpoint,
            heatMode: "heater",
            heating: "off"
        ),
        spa: SpaState(
            on: false,
            active: false,
            temperature: 90,
            setpoint: 100,
            heatMode: "heater",
            heating: "off",
            accessories: ["jets": false]
        ),
        lights: LightState(on: false, mode: nil, availableModes: []),
        auxiliaries: [],
        pump: PumpInfo(pumpType: "vs", running: true, watts: 1200, rpm: 2500, gpm: 40),
        system: SystemInfo(
            controller: "IntelliTouch",
            firmware: "1.0",
            tempUnit: "F",
            airTemperature: 70,
            freezeProtection: false,
            poolSpaSharedPump: true
        )
    )
}

private func expect(_ condition: @autoclosure () -> Bool, _ message: String) {
    if !condition() {
        fputs("FAIL: \(message)\n", stderr)
        exit(1)
    }
}

private func testPendingMutationReappliesUntilVerified() {
    let pending = PendingPoolMutation(
        description: "Pool setpoint 82",
        createdAt: Date(timeIntervalSince1970: 0),
        mutate: { current in
            current.updating(pool: current.pool?.optimisticSetpointChange(82))
        },
        verify: { current in
            current.pool?.setpoint == 82
        }
    )

    let reconciled = reconcileServerSnapshot(
        makeSystem(poolSetpoint: 80),
        pendingMutations: [pending],
        now: Date(timeIntervalSince1970: 1)
    )

    expect(reconciled.system.pool?.setpoint == 82, "pending mutation should be reapplied to server snapshot")
    expect(reconciled.remainingMutations.count == 1, "unverified mutation should remain pending")
}

private func testVerifiedMutationIsDropped() {
    let pending = PendingPoolMutation(
        description: "Pool setpoint 82",
        createdAt: Date(timeIntervalSince1970: 0),
        mutate: { current in
            current.updating(pool: current.pool?.optimisticSetpointChange(82))
        },
        verify: { current in
            current.pool?.setpoint == 82
        }
    )

    let reconciled = reconcileServerSnapshot(
        makeSystem(poolSetpoint: 82),
        pendingMutations: [pending],
        now: Date(timeIntervalSince1970: 1)
    )

    expect(reconciled.system.pool?.setpoint == 82, "verified server state should be preserved")
    expect(reconciled.remainingMutations.isEmpty, "verified mutation should be removed")
}

@main
struct PoolStateMergeTestRunner {
    static func main() {
        testPendingMutationReappliesUntilVerified()
        testVerifiedMutationIsDropped()
        print("PoolStateMergeTests passed")
    }
}

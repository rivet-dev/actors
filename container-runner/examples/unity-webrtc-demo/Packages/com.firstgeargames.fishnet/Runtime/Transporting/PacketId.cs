using FishNet.Documenting;

namespace FishNet.Transporting
{
    /// <summary>
    /// PacketIds to indicate the type of packet which is being sent or arriving.
    /// </summary>
    [APIExclude]
    public enum PacketId : ushort
    {
        Unset = 0,
        // Not used with network traffic statistics.
        Authenticated = 1,
        // Not used with network traffic statistics.
        Split = 2,
        ObjectSpawn = 3,
        ObjectDespawn = 4,
        PredictedSpawnResult = 5,
        SyncType = 7,
        ServerRpc = 8,
        ObserversRpc = 9,
        TargetRpc = 10,
        // Not used with network traffic statistics.
        OwnershipChange = 11,
        Broadcast = 12,
        // Used only as outbound identifier for network traffic statistics - this Id is never transmitted.
        BulkSpawnOrDespawn = 13,
        PingPong = 14,
        Replicate = 15,
        Reconcile = 16,
        // Not used with network traffic statistics.
        Disconnect = 17,
        TimingUpdate = 18,
        UNUSED2 = 19,
        StateUpdate = 20,
        // Not used with network traffic statistics.
        Version = 21
    }
}
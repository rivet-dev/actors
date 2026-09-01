namespace FishNet.Object
{
    /// <summary>
    /// Action to take when despawning a NetworkObject.
    /// </summary>
    public enum DespawnType : byte
    {
        Destroy = 0,
        Pool = 1
    }

    /// <summary>
    /// Current state of the NetworkObject.
    /// </summary>
    internal enum NetworkObjectState : byte
    {
        /// <summary>
        /// State has not been set. This occurs when the object has never been spawned or despawned.
        /// </summary>
        Unset = 0,
        /// <summary>
        /// Object is currently spawned.
        /// </summary>
        Spawned = 1,
        /// <summary>
        /// Object is currently despawned.
        /// </summary>
        Despawned = 2
    }

    /// <summary>
    /// Options on retrieving nested NetworkObjects.
    /// </summary>
    [System.Flags]
    internal enum GetNetworkObjectOption : int
    {
        /// <summary>
        /// Include NetworkObject which nested are being returned for.
        /// </summary>
        Self = 1 << 0,
        /// <summary>
        /// Include initialize nested.
        /// </summary>
        InitializedNested = 1 << 1,
        /// <summary>
        /// Include runtime nested.
        /// </summary>
        RuntimeNested = 1 << 2,
        /// <summary>
        /// Recursively iterate nested includes.
        /// </summary>
        /// <remarks>This only functions if Initialized or Runtime is flagged.</remarks>
        Recursive = 1 << 3,
        /// <summary>
        /// Uses InitializedNested and RuntimeNested flags.
        /// </summary>
        AllNested = InitializedNested | RuntimeNested,
        /// <summary>
        /// Uses InitializedNested, RuntimeNested, and Recursive flags.
        /// </summary>
        AllNestedRecursive = InitializedNested | RuntimeNested | Recursive,
        /// <summary>
        /// Sets all flags.
        /// </summary>
        All = ~0
    }

    internal static class GetNetworkObjectOptionExtensions
    {
        /// <summary>
        /// True if whole contains part.
        /// </summary>
        public static bool FastContains(this GetNetworkObjectOption whole, GetNetworkObjectOption part) => (whole & part) == part;
    }
}
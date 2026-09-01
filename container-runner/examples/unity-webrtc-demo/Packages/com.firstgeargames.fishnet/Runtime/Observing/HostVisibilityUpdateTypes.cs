namespace FishNet.Observing
{
    [System.Flags]
    public enum HostVisibilityUpdateTypes : byte
    {
        /// <summary>
        /// Include this flag to update manager.
        /// </summary>
        Manager = 1,
        /// <summary>
        /// Include this flag to update spawned.
        /// </summary>
        Spawned = 2
    }
}
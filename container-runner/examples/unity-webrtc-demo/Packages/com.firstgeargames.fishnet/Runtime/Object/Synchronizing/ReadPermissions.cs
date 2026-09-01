namespace FishNet.Object.Synchronizing
{
    /// <summary>
    /// Which clients may receive synchronization updates.
    /// </summary>
    /// // Remove on V5. Just rename file to ReadPermission.cs, do not remove.
    public enum ReadPermission : byte
    {
        /// <summary>
        /// All observers will receive updates.
        /// </summary>
        Observers = 0,
        /// <summary>
        /// Only owner will receive updates.
        /// </summary>
        OwnerOnly = 1,
        /// <summary>
        /// Send to all observers except owner.
        /// </summary>
        ExcludeOwner = 2
    }
}
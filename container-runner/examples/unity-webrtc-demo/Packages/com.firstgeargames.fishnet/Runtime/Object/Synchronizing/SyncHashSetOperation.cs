using FishNet.Documenting;

namespace FishNet.Object.Synchronizing
{
    [APIExclude]
    public enum SyncHashSetOperation : byte
    {
        /// <summary>
        /// An item is added to the collection.
        /// </summary>
        Add,
        /// <summary>
        /// An item is removed from the collection.
        /// </summary>
        Remove,
        /// <summary>
        /// Collection is cleared.
        /// </summary>
        Clear,
        /// <summary>
        /// An item has been updated within the collection. This is generally used when modifying data within a container.
        /// </summary>
        Set,
        /// <summary>
        /// All operations for the tick have been processed. This only occurs on clients as the server is unable to be aware of when the user is done modifying the list.
        /// </summary>
        Complete
    }
}
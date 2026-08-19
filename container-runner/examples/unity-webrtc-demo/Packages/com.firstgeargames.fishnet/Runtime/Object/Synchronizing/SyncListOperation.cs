using FishNet.Documenting;

namespace FishNet.Object.Synchronizing
{
    [APIExclude]
    public enum SyncListOperation : byte
    {
        /// <summary>
        /// An item is added to the collection.
        /// </summary>
        Add,
        /// <summary>
        /// An item is inserted into the collection.
        /// </summary>
        Insert,
        /// <summary>
        /// An item is set in the collection.
        /// </summary>
        Set,
        /// <summary>
        /// An item is removed from the collection.
        /// </summary>
        RemoveAt,
        /// <summary>
        /// Collection is cleared.
        /// </summary>
        Clear,
        /// <summary>
        /// All operations for the tick have been processed. This only occurs on clients as the server is unable to be aware of when the user is done modifying the list.
        /// </summary>
        Complete
    }
}
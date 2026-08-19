using FishNet.Documenting;

namespace FishNet.Object.Synchronizing
{
    [APIExclude]
    public enum SyncDictionaryOperation : byte
    {
        /// <summary>
        /// A key and value have been added to the collection.
        /// </summary>
        Add,
        /// <summary>
        /// Collection has been cleared.
        /// </summary>
        Clear,
        /// <summary>
        /// A key was removed from the collection.
        /// </summary>
        Remove,
        /// <summary>
        /// A value has been set for a key in the collection.
        /// </summary>
        Set,
        /// <summary>
        /// All operations for the tick have been processed.
        /// </summary>
        Complete
    }
}
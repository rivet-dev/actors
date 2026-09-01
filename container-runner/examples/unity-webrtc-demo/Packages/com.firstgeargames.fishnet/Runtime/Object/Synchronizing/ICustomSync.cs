namespace FishNet.Object.Synchronizing
{
    /// <summary>
    /// Custom SyncObjects must inherit from SyncBase and implement this interface.
    /// </summary>
    public interface ICustomSync
    {
        /// <summary>
        /// Get the serialized type.
        /// This must return the value type you are synchronizing, for example a struct or class.
        /// If you are not synchronizing a particular value but instead of supported values such as int, bool, ect, then you may return null on this method.
        /// </summary>
        /// <returns></returns>
        object GetSerializedType();
    }
}
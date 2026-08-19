namespace FishNet.Managing
{
    public enum RemoteTimeoutType
    {
        /// <summary>
        /// Disable this feature.
        /// </summary>
        Disabled = 0,
        /// <summary>
        /// Only enable in release builds.
        /// </summary>
        Release = 1,
        /// <summary>
        /// Enable in all builds and editor.
        /// </summary>
        Development = 2
    }
}
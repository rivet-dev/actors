namespace FishNet.Managing.Scened
{
    /// <summary>
    /// How to replace scenes when loading.
    /// </summary>
    public enum ReplaceOption : byte
    {
        /// <summary>
        /// Replace all scenes, online and offline.
        /// </summary>
        All,
        /// <summary>
        /// Only replace scenes loaded using the SceneManager.
        /// </summary>
        OnlineOnly,
        /// <summary>
        /// Do not replace any scenes, additional scenes will be loaded as additive.
        /// </summary>
        None
    }
}
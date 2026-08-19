namespace FishNet.Managing.Scened
{
    /// <summary>
    /// Type of scopes for a scene load or unload.
    /// </summary>
    public enum SceneScopeType : byte
    {
        /// <summary>
        /// Scene action occured for all clients.
        /// </summary>
        Global = 0,
        /// <summary>
        /// Scene action occurred for specified clients.
        /// </summary>
        Connections = 1
    }
}
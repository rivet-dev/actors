using FishNet.Object;

namespace FishNet.Managing.Object
{
    /// <summary>
    /// When using dual prefabs, defines which prefab to spawn for server, and which for clients.
    /// </summary>
    [System.Serializable]
    public struct DualPrefab
    {
        public NetworkObject Server;
        public NetworkObject Client;
    }
}
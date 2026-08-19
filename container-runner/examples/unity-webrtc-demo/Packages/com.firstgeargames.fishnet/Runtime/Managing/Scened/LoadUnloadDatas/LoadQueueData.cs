using FishNet.Connection;
using FishNet.Utility;
using System.Runtime.CompilerServices;

[assembly: InternalsVisibleTo(UtilityConstants.GENERATED_ASSEMBLY_NAME)]

namespace FishNet.Managing.Scened
{
    /// <summary>
    /// Data generated when loading a scene.
    /// </summary>
    public class LoadQueueData
    {
        /// <summary>
        /// Clients which receive this SceneQueueData. If Networked, all clients do. If Connections, only the specified Connections do.
        /// </summary>
        [System.NonSerialized]
        public SceneScopeType ScopeType;
        /// <summary>
        /// Connections to load scenes for. Only valid on the server and when ScopeType is Connections.
        /// </summary>
        [System.NonSerialized]
        public NetworkConnection[] Connections = new NetworkConnection[0];
        /// <summary>
        /// SceneLoadData to use.
        /// </summary>
        public SceneLoadData SceneLoadData = null;
        /// <summary>
        /// Current global scenes.
        /// </summary>
        public string[] GlobalScenes = new string[0];
        /// <summary>
        /// True if to iterate this queue data as server.
        /// </summary>
        [System.NonSerialized]
        public readonly bool AsServer;
        public LoadQueueData() { }

        internal LoadQueueData(SceneScopeType scopeType, NetworkConnection[] conns, SceneLoadData sceneLoadData, string[] globalScenes, bool asServer)
        {
            ScopeType = scopeType;
            Connections = conns;
            SceneLoadData = sceneLoadData;
            GlobalScenes = globalScenes;
            AsServer = asServer;
        }
    }
}
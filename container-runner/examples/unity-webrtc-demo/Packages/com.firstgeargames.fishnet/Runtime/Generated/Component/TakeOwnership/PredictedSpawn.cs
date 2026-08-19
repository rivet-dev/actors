using FishNet.Connection;
using FishNet.Object;
using UnityEngine;

namespace FishNet.Component.Ownership
{
    /// <summary>
    /// Adding this component allows any client to use predictive spawning on this prefab.
    /// </summary>
    public class PredictedSpawn : NetworkBehaviour
    {
        #region Serialized.
        /// <summary>
        /// True to allow clients to predicted spawn this object.
        /// </summary>
        public bool GetAllowSpawning() => _allowSpawning;

        /// <summary>
        /// Sets to allow predicted spawning. This must be set on client and server.
        /// </summary>
        /// <param name = "value">New value.</param>
        public void SetAllowSpawning(bool value) => _allowSpawning = value;

        [Tooltip("True to allow clients to predicted spawn this object.")]
        [SerializeField]
        private bool _allowSpawning = true;

        /// <summary>
        /// True to allow clients to predicted despawn this object.
        /// </summary>
        public bool GetAllowDespawning() => _allowDespawning;

        /// <summary>
        /// Sets to allow predicted despawning. This must be set on client and server.
        /// </summary>
        /// <param name = "value">New value.</param>
        public void SetAllowDespawning(bool value) => _allowDespawning = value;

        [Tooltip("True to allow clients to predicted despawn this object.")]
        [SerializeField]
        private bool _allowDespawning = true;
        #endregion

        /// <summary>
        /// Called on the client when trying to predicted spawn this object.
        /// </summary>
        /// <param name = "owner">Owner specified to spawn with.</param>
        /// <returns>True if able to spawn.</returns>
        public virtual bool OnTrySpawnClient(NetworkConnection owner = null)
        {
            return GetAllowSpawning();
        }

        /// <summary>
        /// Called on the server when a client tries to predicted spawn this object.
        /// </summary>
        /// <param name = "spawner">Connection trying to predicted spawn this object.</param>
        /// <param name = "owner">Owner specified to spawn with.</param>
        /// <returns>True if able to spawn.</returns>
        public virtual bool OnTrySpawnServer(NetworkConnection spawner, NetworkConnection owner = null)
        {
            return GetAllowSpawning();
        }

        /// <summary>
        /// Called on the client when trying to predicted spawn this object.
        /// </summary>
        /// <returns>True if able to despawn.</returns>
        public virtual bool OnTryDespawnClient()
        {
            return GetAllowDespawning();
        }

        /// <summary>
        /// Called on the server when a client tries to predicted despawn this object.
        /// </summary>
        /// <param name = "despawner">Connection trying to predicted despawn this object.</param>
        /// <returns>True if able to despawn.</returns>
        public virtual bool OnTryDespawnServer(NetworkConnection despawner)
        {
            return GetAllowDespawning();
        }
    }
}
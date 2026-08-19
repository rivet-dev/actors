using FishNet.Broadcast;
using FishNet.Managing;
using FishNet.Transporting;
using UnityEngine;

namespace FishNet.Object
{
    public partial class NetworkObject : MonoBehaviour
    {
        /// <summary>
        /// Sends a broadcast to Observers on this NetworkObject.
        /// </summary>
        /// <typeparam name = "T">Type of broadcast to send.</typeparam>
        /// <param name = "message">Broadcast data being sent; for example: an instance of your broadcast type.</param>
        /// <param name = "requireAuthenticated">True if the client must be authenticated for this broadcast to send.</param>
        /// <param name = "channel">Channel to send on.</param>
        public void Broadcast<T>(T message, bool requireAuthenticated = true, Channel channel = Channel.Reliable) where T : struct, IBroadcast
        {
            if (NetworkManager == null)
            {
                NetworkManager.LogWarning($"Cannot send broadcast from {gameObject.name}, NetworkManager reference is null. This may occur if the object is not spawned or initialized.");
                return;
            }

            NetworkManager.ServerManager.Broadcast(Observers, message, requireAuthenticated, channel);
        }
    }
}
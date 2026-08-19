using FishNet.Connection;
using System.Runtime.CompilerServices;
using FishNet.Serializing;
using UnityEngine;

namespace FishNet.Object
{
    public partial class NetworkObject : MonoBehaviour
    {
        #region Private.
        /// <summary>
        /// True if OnStartServer was called.
        /// </summary>
        private bool _onStartServerCalled;
        /// <summary>
        /// True if OnStartClient was called.
        /// </summary>
        private bool _onStartClientCalled;
        #endregion

        // ReSharper disable Unity.PerformanceAnalysis
        /// <summary>
        /// Called after all data is synchronized with this NetworkObject.
        /// </summary>
        private void InvokeStartCallbacks(bool asServer, bool invokeSyncTypeCallbacks)
        {
            /* Note: When invoking OnOwnership here previous owner will
             * always be an empty connection, since the object is just
             * now initializing. */

            // Invoke OnStartNetwork.
            bool invokeOnNetwork = asServer || IsServerOnlyStarted || IsClientOnlyInitialized;
            if (invokeOnNetwork)
            {
                for (int i = 0; i < NetworkBehaviours.Count; i++)
                    NetworkBehaviours[i].InvokeOnNetwork_Internal(start: true);
            }

            //As server.
            if (asServer)
            {
                for (int i = 0; i < NetworkBehaviours.Count; i++)
                    NetworkBehaviours[i].OnStartServer_Internal();
                _onStartServerCalled = true;
                for (int i = 0; i < NetworkBehaviours.Count; i++)
                    NetworkBehaviours[i].OnOwnershipServer_Internal(Managing.NetworkManager.EmptyConnection);
            }
            //As client.
            else
            {
                for (int i = 0; i < NetworkBehaviours.Count; i++)
                    NetworkBehaviours[i].OnStartClient_Internal();
                _onStartClientCalled = true;
                for (int i = 0; i < NetworkBehaviours.Count; i++)
                    NetworkBehaviours[i].OnOwnershipClient_Internal(Managing.NetworkManager.EmptyConnection);
            }

            if (invokeSyncTypeCallbacks)
                InvokeOnStartSyncTypeCallbacks(true);

            InvokeStartCallbacks_Prediction(asServer);
        }

        /// <summary>
        /// Invokes OnStartXXXX for synctypes, letting them know the NetworkBehaviour start cycle has been completed.
        /// </summary>
        internal void InvokeOnStartSyncTypeCallbacks(bool asServer)
        {
            for (int i = 0; i < NetworkBehaviours.Count; i++)
                NetworkBehaviours[i].InvokeSyncTypeOnStartCallbacks(asServer);
        }

        /// <summary>
        /// Invokes OnStopXXXX for synctypes, letting them know the NetworkBehaviour stop cycle is about to start.
        /// </summary>
        internal void InvokeOnStopSyncTypeCallbacks(bool asServer)
        {
            for (int i = 0; i < NetworkBehaviours.Count; i++)
                NetworkBehaviours[i].InvokeSyncTypeOnStopCallbacks(asServer);
        }

        /// <summary>
        /// Invokes events to be called after OnServerStart.
        /// This is made one method to save instruction calls.
        /// </summary>
        /// <param name = ""></param>
        internal void OnSpawnServer(NetworkConnection conn)
        {
            for (int i = 0; i < NetworkBehaviours.Count; i++)
                NetworkBehaviours[i].SendBufferedRpcs(conn);

            for (int i = 0; i < NetworkBehaviours.Count; i++)
                NetworkBehaviours[i].OnSpawnServer(conn);
        }

        /// <summary>
        /// Called on the server before it sends a despawn message to a client.
        /// </summary>
        /// <param name = "conn">Connection spawn was sent to.</param>
        internal void InvokeOnServerDespawn(NetworkConnection conn)
        {
            for (int i = 0; i < NetworkBehaviours.Count; i++)
                NetworkBehaviours[i].OnDespawnServer(conn);
        }

        // ReSharper disable Unity.PerformanceAnalysis
        /// <summary>
        /// Invokes OnStop callbacks.
        /// </summary>
        internal void InvokeStopCallbacks(bool asServer, bool invokeSyncTypeCallbacks)
        {
            InvokeStopCallbacks_Prediction(asServer);

            if (invokeSyncTypeCallbacks)
                InvokeOnStopSyncTypeCallbacks(asServer);

            if (asServer && _onStartServerCalled)
            {
                for (int i = 0; i < NetworkBehaviours.Count; i++)
                    NetworkBehaviours[i].OnStopServer_Internal();

                if (!_onStartClientCalled)
                    InvokeOnNetwork();

                _onStartServerCalled = false;
            }
            else if (!asServer && _onStartClientCalled)
            {
                for (int i = 0; i < NetworkBehaviours.Count; i++)
                    NetworkBehaviours[i].OnStopClient_Internal();

                /* Only invoke OnNetwork if server start isn't called, otherwise
                 * that means this is still intialized on the server. This would
                 * happen if the object despawned for the clientHost but not on the
                 * server. */
                if (!_onStartServerCalled)
                    InvokeOnNetwork();

                _onStartClientCalled = false;
            }

            void InvokeOnNetwork()
            {
                for (int i = 0; i < NetworkBehaviours.Count; i++)
                    NetworkBehaviours[i].InvokeOnNetwork_Internal(start: false);
            }
        }

        /// <summary>
        /// Invokes OnOwnership callbacks when ownership changes.
        /// This is not to be called when assigning ownership during a spawn message.
        /// </summary>
        private void InvokeManualOwnershipChange(NetworkConnection prevOwner, bool asServer)
        {
            if (asServer)
            {
                for (int i = 0; i < NetworkBehaviours.Count; i++)
                    NetworkBehaviours[i].OnOwnershipServer_Internal(prevOwner);

                WriteSyncTypesForManualOwnershipChange(prevOwner);
            }
            else
            {
                /* If local client is owner and not server then only
                 * invoke if the prevOwner is different. This prevents
                 * the owner change callback from happening twice when
                 * using TakeOwnership.
                 *
                 * Further explained, the TakeOwnership sets local client
                 * as owner client-side, which invokes the OnOwnership method.
                 * Then when the server approves the owner change it would invoke
                 * again, which is not needed. */
                bool blockInvoke = IsOwner && !IsServerStarted && prevOwner == Owner;
                if (!blockInvoke)
                {
                    for (int i = 0; i < NetworkBehaviours.Count; i++)
                        NetworkBehaviours[i].OnOwnershipClient_Internal(prevOwner);
                }
            }
        }
    }
}
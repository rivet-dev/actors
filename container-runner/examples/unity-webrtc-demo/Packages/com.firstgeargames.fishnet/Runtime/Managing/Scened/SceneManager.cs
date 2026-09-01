using FishNet.Connection;
using FishNet.Managing.Client;
using FishNet.Managing.Logging;
using FishNet.Managing.Server;
using FishNet.Object;
using FishNet.Serializing.Helping;
using FishNet.Transporting;
using GameKit.Dependencies.Utilities;
using GameKit.Dependencies.Utilities.Types;
using System;
using System.Collections;
using System.Collections.Generic;
using System.Linq;
using UnityEngine;
using UnityEngine.SceneManagement;
using UnityEngine.Serialization;
using UnitySceneManager = UnityEngine.SceneManagement.SceneManager;

namespace FishNet.Managing.Scened
{
    /// <summary>
    /// Handles loading, unloading, and scene visibility for clients.
    /// </summary>
    [DisallowMultipleComponent]
    [AddComponentMenu("FishNet/Manager/SceneManager")]
    public sealed class SceneManager : MonoBehaviour
    {
        #region Types.
        /// <summary>
        /// Used to track scenes which clients have been made aware of to load, but the clients have not yet confirmed to have loaded the scene.
        /// </summary>
        public class PendingClientSceneLoads
        {
            /// <summary>
            /// Scene handles which have clients that have not yet confirmed the loading status.
            /// </summary>
            private Dictionary<int, HashSet<NetworkConnection>> _scenesWithPendingLoads = new();
            /// <summary>
            /// Clients with pending loads, and each scene handle pending.
            /// </summary>
            private Dictionary<NetworkConnection, List<int>> _clientsWithPendingLoads = new();
            /// <summary>
            /// Clients which have been sent the initial scene load with no scenes specified.
            /// </summary>
            public HashSet<NetworkConnection> _clientsWithDefaultLoad = new();
            public bool RemoveClientFromDefaultLoad(NetworkConnection conn) => _clientsWithDefaultLoad.Remove(conn);
            public void AddClientToDefaultLoad(NetworkConnection conn) => _clientsWithDefaultLoad.Add(conn);

            /// <summary>
            /// Adds a pending load for a client.
            /// </summary>
            public void AddClientToScene(NetworkConnection connection, int sceneHandle)
            {
                /* The client has 1 or more pending loads already. See
                 * if the specified scene is already pending. */
                if (_clientsWithPendingLoads.TryGetValueIL2CPP(connection, out List<int> sceneList))
                {
                    //Scene is already marked for the connection.
                    if (sceneList.Contains(sceneHandle))
                        return;
                }
                //No pending loads, make new entry.
                else
                {
                    sceneList = new();
                    _clientsWithPendingLoads.Add(connection, sceneList);
                }

                sceneList.Add(sceneHandle);

                if (!_scenesWithPendingLoads.TryGetValueIL2CPP(sceneHandle, out HashSet<NetworkConnection> connectionsLoadingScene))
                {
                    connectionsLoadingScene = new();
                    _scenesWithPendingLoads.Add(sceneHandle, connectionsLoadingScene);
                }

                connectionsLoadingScene.Add(connection);
            }

            /// <summary>
            /// Removes a client from all pending loads.
            /// </summary
            /// <returns>Scene handles which no longer have clients pending loads.</returns>
            internal List<int> RemoveClientFromAllScenes(NetworkConnection conn)
            {
                List<int> emptyScenes = new();

                if (!_clientsWithPendingLoads.TryGetValueIL2CPP(conn, out List<int> sceneList))
                    return emptyScenes;

                foreach (int sceneHandle in sceneList)
                {
                    /* If the scene is in the clients list then it should
                     * be in scenesWithPendingLoads. This is a safety check but
                     * should never happen. */
                    if (!_scenesWithPendingLoads.TryGetValueIL2CPP(sceneHandle, out HashSet<NetworkConnection> connectionsLoadingScene))
                        continue;

                    connectionsLoadingScene.Remove(conn);
                    //No more connections pending the load.
                    if (connectionsLoadingScene.Count == 0)
                    {
                        emptyScenes.Add(sceneHandle);
                        _scenesWithPendingLoads.Remove(sceneHandle);
                    }
                }

                return emptyScenes;
            }

            /// <summary>
            /// Removes a client from a specific scene.
            /// </summary
            /// <param name="sceneHasNoPendingLoads">Becomes true if the scene which the connection is being removed from has no more pending loads.</param>
            /// <returns>True if the client had the specified scene as pending.</returns>
            internal bool RemoveClientFromScene(NetworkConnection conn, int sceneHandle, out bool sceneHasNoPendingLoads)
            {
                // The scene has does not have any pending clients.
                if (!_scenesWithPendingLoads.TryGetValueIL2CPP(sceneHandle, out HashSet<NetworkConnection> connectionsLoadingScene))
                {
                    sceneHasNoPendingLoads = true;
                    return false;
                }

                // Scene exist but the client count is empty.
                if (connectionsLoadingScene.Count == 0)
                {
                    _scenesWithPendingLoads.Remove(sceneHandle);
                    sceneHasNoPendingLoads = true;
                    return false;
                }

                /* If here then the scene exist with pending clients,
                 * and has at least 1 client in it. */

                /* If client does not have any pending loads then
                 * there is nothing to remove, which means the requested
                 * scene still has clients in it. */
                if (!_clientsWithPendingLoads.TryGetValueIL2CPP(conn, out List<int> sceneList))
                {
                    sceneHasNoPendingLoads = false;
                    return false;
                }

                /* If the client is not pending the scene load then
                 * still nothing has changed. */
                if (!sceneList.Contains(sceneHandle))
                {
                    sceneHasNoPendingLoads = false;
                    return false;
                }

                /* If here the client does have the scene as
                 * pending load. */
                bool removedFromClientsPendingList = sceneList.Remove(sceneHandle);
                //No more scenes pending load.
                if (sceneList.Count == 0)
                    _clientsWithPendingLoads.Remove(conn);

                // Remove conn from scenes with pending loads.
                connectionsLoadingScene.Remove(conn);
                if (connectionsLoadingScene.Count == 0)
                {
                    _scenesWithPendingLoads.Remove(sceneHandle);
                    sceneHasNoPendingLoads = true;
                }
                //Connections are still loading the scene. Else statement for clarity.
                else
                {
                    sceneHasNoPendingLoads = false;
                }

                return removedFromClientsPendingList;
            }

            /// <summary>
            /// Returns if a scene has any number of clients still pending load.
            /// </summary>
            internal bool HasSceneAnyPendingLoads(int sceneHandle) => _scenesWithPendingLoads.TryGetValueIL2CPP(sceneHandle, out _);

            /// <summary>
            /// Clears all information.
            /// </summary>
            public void ResetState()
            {
                _scenesWithPendingLoads.Clear();
                _clientsWithPendingLoads.Clear();
                _clientsWithDefaultLoad.Clear();
            }
        }

        internal enum LightProbeUpdateType
        {
            Asynchronous = 0,
            BlockThread = 1,
            Off = 2
        }
        #endregion

        #region Public.
        /// <summary>
        /// Called after the active scene has been set, immediately after scene loads. This will occur before NetworkBehaviour callbacks run for the scene's objects.
        /// The boolean will indicate if the scene set active was specified by the user.
        /// </summary>
        public event Action<bool> OnActiveSceneSet;
        /// <summary>
        /// Called when a client loads initial scenes after connecting. Boolean will be true if asServer. This will invoke even if the SceneManager is not used when the client completes fully connecting to the server.
        /// </summary>
        public event Action<NetworkConnection, bool> OnClientLoadedStartScenes;
        /// <summary>
        /// Called when a scene change queue has begun. This will only call if a scene has succesfully begun to load or unload. The queue may process any number of scene events. For example: if a scene is told to unload while a load is still in progress, then the unload will be placed in the queue.
        /// </summary>
        public event Action OnQueueStart;
        /// <summary>
        /// Called when the scene queue is emptied.
        /// </summary>
        public event Action OnQueueEnd;
        /// <summary>
        /// Called when a scene load starts.
        /// </summary>
        public event Action<SceneLoadStartEventArgs> OnLoadStart;
        /// <summary>
        /// Called when completion percentage changes while loading a scene. Value is between 0f and 1f, while 1f is 100% done. Can be used for custom progress bars when loading scenes.
        /// </summary>
        public event Action<SceneLoadPercentEventArgs> OnLoadPercentChange;
        /// <summary>
        /// Called when a scene load ends.
        /// </summary>
        public event Action<SceneLoadEndEventArgs> OnLoadEnd;
        /// <summary>
        /// Called when a scene unload starts.
        /// </summary>
        public event Action<SceneUnloadStartEventArgs> OnUnloadStart;
        /// <summary>
        /// Called when a scene unload ends.
        /// </summary>
        public event Action<SceneUnloadEndEventArgs> OnUnloadEnd;
        /// <summary>
        /// Called when a client presence changes within a scene, before the server rebuilds observers.
        /// </summary>
        public event Action<ClientPresenceChangeEventArgs> OnClientPresenceChangeStart;
        /// <summary>
        /// Called when a client presence changes within a scene, after the server rebuilds observers.
        /// </summary>
        public event Action<ClientPresenceChangeEventArgs> OnClientPresenceChangeEnd;
        /// <summary>
        /// Connections within each scene.
        /// </summary>
        public Dictionary<Scene, HashSet<NetworkConnection>> SceneConnections { get; private set; } = new();
        /// <summary>
        /// </summary>
        [Tooltip("Script to handle addressables loading and unloading. This field may be blank if addressables are not being used.")]
        [SerializeField]
        private SceneProcessorBase _sceneProcessor;

        /// <summary>
        /// Script to handle addressables loading and unloading. This field may be blank if addressables are not being used.
        /// </summary>
        /// <returns></returns>
        public SceneProcessorBase GetSceneProcessor() => _sceneProcessor;

        /// <summary>
        /// Sets the SceneProcessor to use.
        /// </summary>
        /// <param name = "value"></param>
        public void SetSceneProcessor(SceneProcessorBase value) => _sceneProcessor = value;

        /// <summary>
        /// NetworkManager for this script.
        /// </summary>
        public NetworkManager NetworkManager { get; private set; }
        #endregion

        #region Internal.
        /// <summary>
        /// Called after the active scene has been set, immediately after scene loads.
        /// </summary>
        internal event Action<bool> OnActiveSceneSetInternal;
        /// <summary>
        /// True if the SceneManager has items in queue.
        /// </summary>
        internal bool IteratingQueue { get; private set; }
        /// <summary>
        /// Unscaled time when the SceneManager completed its last queue.
        /// </summary>
        internal float QueueCompleteTime { get; private set; }

        /// <summary>
        /// True if currently IteratingQueue or queue has completed recently.
        /// </summary>
        internal bool HasProcessedScenesRecently(float timeFrame)
        {
            if (IteratingQueue || Time.unscaledTime - QueueCompleteTime < timeFrame)
                return true;

            return false;
        }
        #endregion

        #region Serialized.
        /// <summary>
        /// How to update light probes after loading or unloading scenes.
        /// </summary>
        [Tooltip("How to update light probes after loading or unloading scenes.")]
        [SerializeField]
        private LightProbeUpdateType _lightProbeUpdating = LightProbeUpdateType.Asynchronous;
        /// <summary>
        /// True to move spawned objects visible to the client that are within an unloading scene. This ensures the objects are despawned on the client side rather than when the scene is destroyed.
        /// </summary>
        [FormerlySerializedAs("_moveClientHostObjects")]
        [Tooltip("True to move spawned objects visible to the client that are within an unloading scene. This ensures the objects are despawned on the client side rather than when the scene is destroyed.")]
        [SerializeField]
        private bool _moveClientObjects = true;

        /// <summary>
        /// Sets a new value for MoveClientObjects.
        /// </summary>
        public void SetMoveClientObjects(bool value) => _moveClientObjects = value;

        /// <summary>
        /// True to automatically set active scenes when loading and unloading scenes.
        /// </summary>
        [Tooltip("True to automatically set active scenes when loading and unloading scenes.")]
        [SerializeField]
        private bool _setActiveScene = true;
        #endregion

        #region Private.
        /// <summary>
        /// ServerManager for this script.
        /// </summary>
        private ServerManager _serverManager => NetworkManager.ServerManager;
        /// <summary>
        /// ClientManager for this script.
        /// </summary>
        private ClientManager _clientManager => NetworkManager.ClientManager;
        /// <summary>
        /// Scenes which are currently loaded as networked scenes. All players should have networked scenes loaded.
        /// </summary>
        private string[] _globalScenes = new string[0];
        /// <summary>
        /// Lastest SceneLoadData for a global load.
        /// </summary>
        private SceneLoadData _globalSceneLoadData = new();
        /// <summary>
        /// Scenes to load or unload, in order.
        /// </summary>
        private List<object> _queuedOperations = new();
        /// <summary>
        /// Scenes which must be manually unloaded, even when emptied.
        /// </summary>
        private HashSet<Scene> _manualUnloadScenes = new();
        /// <summary>
        /// Contains information and actions about scenes a client has been told to load.
        /// </summary>
        private PendingClientSceneLoads _pendingClientSceneLoads = new();
        /// <summary>
        /// Scene containing moved objects when changing single scene. On client this will contain all objects moved until the server destroys them.
        /// The network only sends spawn messages once per-client, per server side scene load. If a scene load is performed only for specific connections
        /// then the server is not resetting their single scene, but rather the single scene for those connections only. Because of this, any objects
        /// which are to be moved will not receive a second respawn message, as they are never destroyed on server, only on client.
        /// While on server only this scene contains objects being moved temporarily, before being moved to the new scene.
        /// </summary>
        private Scene _movedObjectsScene;
        /// <summary>
        /// Scene containing objects awaiting to be destroyed by the client-host.
        /// This is required when unloading scenes where the client-host has visibility.
        /// Otherwise the objects would become destroyed when the scene unloads on the server
        /// which would cause missing networkobjects on clients when receiving despawn messages.
        /// </summary>
        private Scene _delayedDestroyScene;
        /// <summary>
        /// A scene to be set as the active scene where there are no global scenes.
        /// This is used to prevent connection scenes and MovedObjectsScene from becoming the active scene.
        /// </summary>
        private Scene _fallbackActiveScene;
        /// <summary>
        /// Becomes true when when a scene first successfully begins to load or unload. Value is reset to false when the scene queue is emptied.
        /// </summary>
        private bool _sceneQueueStartInvoked;
        /// <summary>
        /// Objects being moved from MovedObjects scene to another.
        /// </summary>
        private List<GameObject> _movingObjects = new();
        /// <summary>
        /// How many scene load confirmations the server is expecting from a client.
        /// Unloads do not need to be checked because server does not require confirmation for those.
        /// This is used to prevent attacks.
        /// </summary>
        //private Dictionary<NetworkConnection, int> _pendingClientSceneChanges = new();
        ///// <summary>
        ///// Cache of SceneLookupData.
        ///// </summary>
        // private SceneLookupData _sceneLookupDataCache = new SceneLookupData();
        /// <summary>
        /// GlobalScenes currently loading on the server.
        /// </summary>
        private HashSet<string> _serverGlobalScenesLoading = new();
        #endregion

        #region Consts.
        /// <summary>
        /// String to use when scene data used to load is invalid.
        /// </summary>
        private const string INVALID_SCENELOADDATA = "One or more datas in SceneLoadData are invalid.This generally occurs when calling this method without specifying any scenes or when data fields are null.";
        /// <summary>
        /// String to use when scene data used to unload is invalid.
        /// </summary>
        private const string INVALID_SCENEUNLOADDATA = "One or more datas in SceneLoadData are invalid.This generally occurs when calling this method without specifying any scenes or when data fields are null.";
        #endregion

        #region Unity callbacks and initialization.
        private void Awake()
        {
            UnitySceneManager.sceneUnloaded += SceneManager_SceneUnloaded;
            if (_sceneProcessor == null)
                _sceneProcessor = gameObject.AddComponent<DefaultSceneProcessor>();
            _sceneProcessor.Initialize(this);
        }

        private void OnDestroy()
        {
            UnitySceneManager.sceneUnloaded -= SceneManager_SceneUnloaded;
        }

        /// <summary>
        /// Called when the server connection state changes.
        /// </summary>
        private void ServerManager_OnServerConnectionState(ServerConnectionStateArgs obj)
        {
            // If no servers are started.
            if (!NetworkManager.ServerManager.IsAnyServerStarted())
                ResetValues();
        }

        /// <summary>
        /// Resets as if first use.
        /// </summary>
        private void ResetValues()
        {
            SceneConnections.Clear();
            _globalScenes = new string[0];
            _globalSceneLoadData = new();
            _queuedOperations.Clear();
            _manualUnloadScenes.Clear();
            _sceneQueueStartInvoked = false;
            _movingObjects.Clear();
        }

        /// <summary>
        /// Called when a connection state changes for a remote client.
        /// </summary>
        private void ServerManager_OnRemoteConnectionState(NetworkConnection arg1, RemoteConnectionStateArgs arg2)
        {
            if (arg2.ConnectionState == RemoteConnectionState.Stopped)
                ClientDisconnected(arg1);
        }

        /// <summary>
        /// Initializes this script for use.
        /// </summary>
        /// <param name = "manager"></param>
        internal void InitializeOnce_Internal(NetworkManager manager)
        {
            NetworkManager = manager;
            // No need to unregister since managers are on the same object.
            _serverManager.OnRemoteConnectionState += ServerManager_OnRemoteConnectionState;
            _serverManager.OnServerConnectionState += ServerManager_OnServerConnectionState;
            _clientManager.RegisterBroadcast<LoadScenesBroadcast>(OnServerLoadedScenes);
            _clientManager.RegisterBroadcast<UnloadScenesBroadcast>(OnServerUnloadedScenes);
            _serverManager.RegisterBroadcast<ClientScenesLoadedBroadcast>(OnClientLoadedScenes);
            _serverManager.RegisterBroadcast<EmptyStartScenesBroadcast>(OnClientSentEmptyStartScenes);
            _clientManager.RegisterBroadcast<EmptyStartScenesBroadcast>(OnServerSentEmptyStartScenes);
        }

        /// <summary>
        /// Received when a scene is unloaded.
        /// </summary>
        /// <param name = "arg0"></param>
        private void SceneManager_SceneUnloaded(Scene scene)
        {
            if (!NetworkManager.IsServerStarted)
                return;

            /* Remove any unloaded scenes from local variables. This shouldn't
             * be needed if the user properly utilizes this scene manager,
             * but just incase, we don't want a memory leak. */
            SceneConnections.Remove(scene);
            _manualUnloadScenes.Remove(scene);
            RemoveFromGlobalScenes(scene);
        }
        #endregion

        #region Initial synchronizing.
        /// <summary>
        /// Invokes OnClientLoadedStartScenes if connection just loaded start scenes.
        /// </summary>
        /// <param name = "connection"></param>
        private void TryInvokeLoadedStartScenes(NetworkConnection connection, bool asServer)
        {
            if (connection.SetLoadedStartScenes(asServer))
                OnClientLoadedStartScenes?.Invoke(connection, asServer);
        }

        /// <summary>
        /// Called when authenitcator has concluded a result for a connection. Boolean is true if authentication passed, false if failed. This invokes before OnClientAuthenticated so FishNet may run operations on authenticated clients before user code does.
        /// </summary>
        /// <param name = "obj"></param>
        internal void OnClientAuthenticated(NetworkConnection connection)
        {
            // No global scenes to load.
            if (_globalScenes.Length == 0)
            {
                SendEmptyBroadcast();
                return;
            }

            string[] globalsNotLoading = GlobalScenesExcludingLoading();

            //There are no global scenes which are fully loaded.
            if (globalsNotLoading == null || globalsNotLoading.Length == 0)
            {
                SendEmptyBroadcast();
                return;
            }

            //Build lookupData for each global.
            List<SceneLookupData> sceneLookupData = new();
            List<string> missingGlobalScenes = CollectionCaches<string>.RetrieveList();

            foreach (string s in globalsNotLoading)
            {
                Scene gS = UnitySceneManager.GetSceneByName(s);
                if (gS.IsValid())
                    sceneLookupData.Add(new(gS));
                else
                    missingGlobalScenes.Add(s);
            }

            //One or more global scenes could not be found.
            if (missingGlobalScenes.Count > 0)
                NetworkManager.LogWarning($"The following global scenes were specified but could not be found: {string.Join(", ", missingGlobalScenes)}");

            CollectionCaches<string>.Store(missingGlobalScenes);

            //No global scenes found.
            if (sceneLookupData.Count == 0)
            {
                SendEmptyBroadcast();
                return;
            }

            void SendEmptyBroadcast()
            {
                _pendingClientSceneLoads.AddClientToDefaultLoad(connection);
                /* Invoke that client had loaded the default scenes immediately,
                 * since there are no scenes to load. */
                // OnClientLoadedScenes(connection, new ClientScenesLoadedBroadcast());
                // Tell the client there are no scenes to load.
                EmptyStartScenesBroadcast emptyMsg = new();
                connection.Broadcast(emptyMsg);
            }

            //There is at least 1 global scene to send.
            SceneLoadData sld = new(sceneLookupData.ToArray());
            sld.Params = _globalSceneLoadData.Params;
            sld.Options = _globalSceneLoadData.Options;
            sld.ReplaceScenes = _globalSceneLoadData.ReplaceScenes;
            sld.PreferredActiveScene = _globalSceneLoadData.PreferredActiveScene;

            LoadQueueData qd = new(SceneScopeType.Global, Array.Empty<NetworkConnection>(), sld, _globalScenes, false);
            // Send message to load the networked scenes.
            LoadScenesBroadcast msg = new()
            {
                QueueData = qd
            };

            foreach (SceneLookupData lookupData in sceneLookupData)
                _pendingClientSceneLoads.AddClientToScene(connection, lookupData.Handle);

            connection.Broadcast(msg, requireAuthenticated: true);
        }

        /// <summary>
        /// Received on client when the server has no start scenes.
        /// </summary>
        private void OnServerSentEmptyStartScenes(EmptyStartScenesBroadcast msg, Channel channel)
        {
            TryInvokeLoadedStartScenes(_clientManager.Connection, false);
            _clientManager.Broadcast(msg);
        }

        /// <summary>
        /// Received on server when client confirms there are no start scenes.
        /// </summary>
        private void OnClientSentEmptyStartScenes(NetworkConnection conn, EmptyStartScenesBroadcast msg, Channel channel)
        {
            if (!conn.IsActive)
                return;

            // Already received, shouldn't be happening again.
            if (conn.LoadedStartScenes(true))
                conn.Kick(KickReason.ExploitAttempt, LoggingType.Common, $"Received multiple EmptyStartSceneBroadcast from connectionId {conn.ClientId}. Connection will be kicked immediately.");
            else
                OnClientLoadedScenes(conn, new(), channel);
        }
        #endregion

        #region Player disconnect.
        /// <summary>
        /// Received when a player disconnects from the server.
        /// </summary>
        /// <param name = "conn"></param>
        /// // finish.
        private void ClientDisconnected(NetworkConnection conn)
        {
            _pendingClientSceneLoads.RemoveClientFromAllScenes(conn);
            /* Remove connection from all scenes. While doing so check
             * if scene should be unloaded provided there are no more clients
             * in the scene, and it's set to automatically unload. This situation is a bit
             * unique since a client disconnect happens outside the manager, so there
             * isn't much code we can re-use to perform this operation. */
            List<Scene> scenesToUnload = new();
            // Current active scene.
            Scene activeScene = UnitySceneManager.GetActiveScene();
            foreach (KeyValuePair<Scene, HashSet<NetworkConnection>> item in SceneConnections)
            {
                Scene scene = item.Key;
                HashSet<NetworkConnection> hs = item.Value;

                bool removed = hs.Remove(conn);

                /* Checks split for readability. */

                /* True if SceneConnections has no more connections
                 * in its scene, as well if the scene checked is in
                 * has no other clients pending load confirmation. */
                bool isSceneNowEmpty = removed && hs.Count == 0 && !_pendingClientSceneLoads.HasSceneAnyPendingLoads(SceneHandleCompatibility.ToInt(scene));

                //True if not a global scene and not in scenes to be manually unloaded.
                bool notGlobalAndNotManualUnload = !IsGlobalScene(scene) && !_manualUnloadScenes.Contains(scene);

                //True if not the active scene.
                bool isActiveScene = scene != activeScene;

                /* If no more observers for scene, not a global scene, and not to be manually unloaded
                 * then remove scene from SceneConnections and unload it. */
                if (isSceneNowEmpty && notGlobalAndNotManualUnload && isActiveScene)
                    scenesToUnload.Add(scene);
            }

            // If scenes should be unloaded.
            if (scenesToUnload.Count > 0)
            {
                foreach (Scene s in scenesToUnload)
                    SceneConnections.Remove(s);

                SceneUnloadData sud = new(SceneLookupData.CreateData(scenesToUnload));
                UnloadConnectionScenes(Array.Empty<NetworkConnection>(), sud);
            }
        }
        #endregion

        #region Server received messages.
        /// <summary>
        /// Received on server when a client loads scenes.
        /// </summary>
        /// <param name = "conn"></param>
        /// <param name = "msg"></param>
        private void OnClientLoadedScenes(NetworkConnection conn, ClientScenesLoadedBroadcast msg, Channel channel)
        {
            if (!conn.IsActive)
            {
                _pendingClientSceneLoads.RemoveClientFromAllScenes(conn);
                return;
            }

            if (Comparers.IsDefault(msg))
            {
                /* Msg can only be default when loading initial scenes
                 * If client is not loading initial then kick. */
                if (!_pendingClientSceneLoads.RemoveClientFromDefaultLoad(conn))
                {
                    KickClient($"Default load response received, but client was not sent a default load.");
                    return;
                }
            }
            //Make sure each scene is in pending.
            else
            {
                foreach (SceneLookupData item in msg.SceneLookupDatas)
                {
                    //Make sure the sceneId is pending.
                    if (!_pendingClientSceneLoads.RemoveClientFromScene(conn, item.Handle, out _))
                    {
                        KickClient($"Client {conn.ToString()} sent a scene load response for handle [{item.Handle}], but client was not sent a load for this handle.");
                        break;
                    }

                    Scene s = item.GetScene(out _);
                    if (s.IsValid())
                        AddConnectionToScene(conn, s);
                }
            }

            TryInvokeLoadedStartScenes(conn, true);

            void KickClient(string reason)
            {
                conn.Kick(KickReason.ExploitAttempt, LoggingType.Common, $"{reason} Connection will be kicked immediately.");
            }
        }
        #endregion

        #region Events.
        /// <summary>
        /// Checks if OnQueueStart should invoke, and if so invokes.
        /// </summary>
        private void TryInvokeOnQueueStart()
        {
            if (_sceneQueueStartInvoked)
                return;

            _sceneQueueStartInvoked = true;
            IteratingQueue = true;
            OnQueueStart?.Invoke();
        }

        /// <summary>
        /// Checks if OnQueueEnd should invoke, and if so invokes.
        /// </summary>
        private void TryInvokeOnQueueEnd()
        {
            if (!_sceneQueueStartInvoked)
                return;

            _sceneQueueStartInvoked = false;
            IteratingQueue = false;
            QueueCompleteTime = Time.unscaledTime;
            OnQueueEnd?.Invoke();
        }

        /// <summary>
        /// Invokes that a scene load has started. Only called when valid scenes will be loaded.
        /// </summary>
        /// <param name = "qd"></param>
        private void InvokeOnSceneLoadStart(LoadQueueData qd)
        {
            TryInvokeOnQueueStart();
            OnLoadStart?.Invoke(new(qd));
        }

        /// <summary>
        /// Invokes that a scene load has ended. Only called after a valid scene has loaded.
        /// </summary>
        /// <param name = "qd"></param>
        private void InvokeOnSceneLoadEnd(LoadQueueData qd, List<string> requestedLoadScenes, List<Scene> loadedScenes, string[] unloadedSceneNames)
        {
            // Make new list to not destroy original data.
            List<string> skippedScenes = requestedLoadScenes.ToList();
            // Remove loaded scenes from requested scenes.
            for (int i = 0; i < loadedScenes.Count; i++)
                skippedScenes.Remove(loadedScenes[i].name);

            SceneLoadEndEventArgs args = new(qd, skippedScenes.ToArray(), loadedScenes.ToArray(), unloadedSceneNames);
            OnLoadEnd?.Invoke(args);
        }

        /// <summary>
        /// Invokes that a scene unload has started. Only called when valid scenes will be unloaded.
        /// </summary>
        /// <param name = "sqd"></param>
        private void InvokeOnSceneUnloadStart(UnloadQueueData sqd)
        {
            TryInvokeOnQueueStart();
            OnUnloadStart?.Invoke(new(sqd));
        }

        /// <summary>
        /// Invokes that a scene unload has ended. Only called after a valid scene has unloaded.
        /// </summary>
        /// <param name = "sqd"></param>
        private void InvokeOnSceneUnloadEnd(UnloadQueueData sqd, List<Scene> unloadedScenes, List<UnloadedScene> newUnloadedScenes)
        {
            SceneUnloadEndEventArgs args = new(sqd, unloadedScenes, newUnloadedScenes);
            OnUnloadEnd?.Invoke(args);
        }

        /// <summary>
        /// Invokes when completion percentage changes while unloading or unloading a scene. Value is between 0f and 1f, while 1f is 100% done.
        /// </summary>
        /// <param name = "value"></param>
        private void InvokeOnScenePercentChange(LoadQueueData qd, float value)
        {
            value = Mathf.Clamp(value, 0f, 1f);
            SceneLoadPercentEventArgs slp = new(qd, value);
            OnLoadPercentChange?.Invoke(slp);
        }
        #endregion

        #region Scene queue processing.
        /// <summary>
        /// Queues a load or unload operation and starts queue if needed.
        /// </summary>
        /// <param name = "data"></param>
        private void QueueOperation(object data)
        {
            // Add to scene queue data.        
            _queuedOperations.Add(data);
            /* If only one entry then scene operations are not currently in progress.
             * Should there be more than one entry then scene operations are already
             * occuring. The coroutine will automatically load in order. */

            if (_queuedOperations.Count == 1)
                StartCoroutine(__ProcessSceneQueue());
        }

        /// <summary>
        /// Processes queued scene operations.
        /// </summary>
        /// <param name = "asServer"></param>
        /// <returns></returns>
        private IEnumerator __ProcessSceneQueue()
        {
            /* Queue start won't invoke unless a scene load or unload actually occurs.
             * For example: if a scene is already loaded, and nothing needs to be loaded,
             * queue start will not invoke. */

            while (_queuedOperations.Count > 0)
            {
                // If a load scene.
                if (_queuedOperations[0] is LoadQueueData)
                    yield return StartCoroutine(__LoadScenes());
                // If an unload scene.
                else if (_queuedOperations[0] is UnloadQueueData)
                    yield return StartCoroutine(__UnloadScenes());

                if (_queuedOperations.Count > 0)
                    _queuedOperations.RemoveAt(0);
            }

            TryInvokeOnQueueEnd();
        }
        #endregion

        /// <summary>
        /// Returns global scenes which are not currently being loaded by the server.
        /// </summary>
        /// <returns></returns>
        private string[] GlobalScenesExcludingLoading()
        {
            HashSet<string> excludedScenes = null;
            foreach (string gs in _globalScenes)
            {
                if (_serverGlobalScenesLoading.Contains(gs))
                {
                    if (excludedScenes == null)
                        excludedScenes = new();

                    excludedScenes.Add(gs);
                }
            }

            // Some scenes are excluded.
            if (excludedScenes != null)
            {
                // All are excluded, quick exit to save perf.
                int remaining = _globalScenes.Length - excludedScenes.Count;
                if (remaining <= 0)
                    return null;
                // Some are excluded.
                List<string> results = new();
                foreach (string globalScene in _globalScenes)
                {
                    if (!excludedScenes.Contains(globalScene))
                        results.Add(globalScene);
                }

                return results.ToArray();
            }
            // No scenes are excluded.
            else
            {
                return _globalScenes;
            }
        }

        #region LoadScenes
        /// <summary>
        /// Loads scenes on the server and for all clients. Future clients will automatically load these scenes.
        /// </summary>
        /// <param name = "sceneLoadData">Data about which scenes to load.</param>
        public void LoadGlobalScenes(SceneLoadData sceneLoadData)
        {
            LoadGlobalScenes_Internal(sceneLoadData, _globalScenes, true);
        }

        private void LoadGlobalScenes_Internal(SceneLoadData sceneLoadData, string[] globalScenes, bool asServer)
        {
            if (!CanExecute(asServer, true))
                return;
            if (SceneDataInvalid(sceneLoadData, true))
                return;
            if (sceneLoadData.Options.AllowStacking)
            {
                NetworkManager.LogError($"Stacking scenes is not allowed with Global scenes.");
                return;
            }

            LoadQueueData lqd = new(SceneScopeType.Global, Array.Empty<NetworkConnection>(), sceneLoadData, globalScenes, asServer);
            QueueOperation(lqd);
        }

        /// <summary>
        /// Loads scenes on server and tells connections to load them as well. Other connections will not load this scene.
        /// </summary>
        /// <param name = "conn">Connections to load scenes for.</param>
        /// <param name = "sceneLoadData">Data about which scenes to load.</param>
        public void LoadConnectionScenes(NetworkConnection conn, SceneLoadData sceneLoadData)
        {
            // This cannot use cache because the array will persist for many frames after this method completion.
            LoadConnectionScenes(new NetworkConnection[] { conn }, sceneLoadData);
        }

        /// <summary>
        /// Loads scenes on server and tells connections to load them as well. Other connections will not load this scene.
        /// </summary>
        /// <param name = "conns">Connections to load scenes for.</param>
        /// <param name = "sceneLoadData">Data about which scenes to load.</param>
        public void LoadConnectionScenes(NetworkConnection[] conns, SceneLoadData sceneLoadData)
        {
            LoadConnectionScenes_Internal(conns, sceneLoadData, _globalScenes, true);
        }

        /// <summary>
        /// Loads scenes on server without telling clients to load the scenes.
        /// </summary>
        /// <param name = "sceneLoadData">Data about which scenes to load.</param>
        public void LoadConnectionScenes(SceneLoadData sceneLoadData)
        {
            LoadConnectionScenes_Internal(Array.Empty<NetworkConnection>(), sceneLoadData, _globalScenes, true);
        }

        private void LoadConnectionScenes_Internal(NetworkConnection[] conns, SceneLoadData sceneLoadData, string[] globalScenes, bool asServer)
        {
            if (!CanExecute(asServer, true))
                return;
            if (SceneDataInvalid(sceneLoadData, true))
                return;

            LoadQueueData lqd = new(SceneScopeType.Connections, conns, sceneLoadData, globalScenes, asServer);
            QueueOperation(lqd);
        }

        /// <summary>
        /// Returns if a NetworkObject can be moved.
        /// </summary>
        /// <param name = "warn"></param>
        /// <returns></returns>
        private bool CanMoveNetworkObject(NetworkObject nob, bool warn)
        {
            // Null.
            if (nob == null)
                return WarnAndReturnFalse($"NetworkObject is null.");
            // Not networked.
            if (!nob.GetIsNetworked())
                return WarnAndReturnFalse($"NetworkObject {nob.name} cannot be moved as it is not networked.");
            // Not spawned.
            if (!nob.IsSpawned)
                return WarnAndReturnFalse($"NetworkObject {nob.name} canot be moved as it is not spawned.");
            // SceneObject.
            if (nob.IsSceneObject)
                return WarnAndReturnFalse($"NetworkObject {nob.name} cannot be moved as it is a scene object.");
            // Not root.
            if (nob.transform.parent != null)
                return WarnAndReturnFalse($"NetworkObject {nob.name} cannot be moved because it is not the root object. Unity can only move root objects between scenes.");
            // In DDOL and IsGlobal.
            if (nob.IsGlobal && nob.gameObject.scene.name == DDOL.GetDDOL().gameObject.scene.name)
                return WarnAndReturnFalse($"NetworkObject {nob.name} cannot be moved because it is global. Global objects must remain in the DontDestroyOnLoad scene.");

            // Fall through success.
            return true;

            bool WarnAndReturnFalse(string msg)
            {
                if (warn)
                    NetworkManager.LogWarning(msg);
                return false;
            }
        }

        /// <summary>
        /// Loads a connection scene queue data. This behaves just like a networked scene load except it sends only to the specified connections, and it always loads as an additive scene on server.
        /// </summary>
        /// <returns></returns>
        private IEnumerator __LoadScenes()
        {
            try
            {
                LoadQueueData data = _queuedOperations[0] as LoadQueueData;
                SceneLoadData sceneLoadData = data.SceneLoadData;
                // True if running as server.
                bool asServer = data.AsServer;
                // True if running as client, while network server is active.
                bool asHost = !asServer && NetworkManager.IsServerStarted;

                // If connection went inactive.
                if (!ConnectionActive(asServer))
                    yield break;

                /* Scene sanity checks. */
                if (sceneLoadData.SceneLookupDatas.Length == 0)
                {
                    NetworkManager.LogWarning($"No scenes specified to load.");
                    yield break;
                }

                // True if replacing scenes with specified ones.
                ReplaceOption replaceScenes = sceneLoadData.ReplaceScenes;

                // May be unset if on server, this is fine.
                NetworkConnection localConnection = NetworkManager.ClientManager.Connection;
                /* Immediately set new global scenes. If on client this is whatever
                 * server passes in. This should be set even if scope type
                 * is not global because clients might get a connection scene first.
                 */
                if (!asServer)
                {
                    if (!asHost)
                        _globalScenes = data.GlobalScenes;
                }
                /* However, if server, then only update global scenes if scope
                 * is global. */
                else if (asServer && data.ScopeType == SceneScopeType.Global)
                {
                    _globalSceneLoadData = sceneLoadData;
                    string[] names = sceneLoadData.SceneLookupDatas.GetNames();
                    // Add to server global scenes which are currently loading.
                    foreach (string item in names)
                        _serverGlobalScenesLoading.Add(item);
                    // If replacing.
                    if (replaceScenes != ReplaceOption.None)
                    {
                        _globalScenes = names;
                    }
                    // Add onto.
                    else
                    {
                        int index = _globalScenes.Length;
                        Array.Resize(ref _globalScenes, _globalScenes.Length + names.Length);
                        Array.Copy(names, 0, _globalScenes, index, names.Length);
                    }
                    CheckForDuplicateGlobalSceneNames();
                    data.GlobalScenes = _globalScenes;
                }


                /* Scene queue data scenes.
                 * All scenes in the scene queue data whether they will be loaded or not. */
                List<string> requestedLoadSceneNames = new();
                List<int> requestedLoadSceneHandles = new();

                /* Make a null filled array. This will be populated
                 * using loaded scenes, or already loaded (eg cannot be loaded) scenes. */
                SceneLookupData[] broadcastLookupData = new SceneLookupData[sceneLoadData.SceneLookupDatas.Length];

                /* LoadableScenes and SceneReferenceDatas.
                /* Will contain scenes which may be loaded.
                 * Scenes might not be added to loadableScenes
                 * if for example loadOnlyUnloaded is true and
                 * the scene is already loaded. */
                List<SceneLookupData> loadableScenes = new();
                for (int i = 0; i < sceneLoadData.SceneLookupDatas.Length; i++)
                {
                    SceneLookupData lookupData = sceneLoadData.SceneLookupDatas[i];
                    // Scene to load.
                    bool byHandle;
                    Scene s = lookupData.GetScene(out byHandle);
                    // If found then add it to requestedLoadScenes.
                    if (s.IsValid())
                    {
                        requestedLoadSceneNames.Add(s.name);
                        if (byHandle)
                            requestedLoadSceneHandles.Add(SceneHandleCompatibility.ToInt(s));
                    }

                    if (CanLoadScene(data, lookupData))
                    {
                        // Don't load if as host, server side would have loaded already.
                        if (!asHost)
                            loadableScenes.Add(lookupData);
                    }
                    // Only the server needs to find scene handles to send to client. Client will send these back to the server.
                    else if (asServer)
                    {
                        /* If here then scene cannot be loaded, which
                         * can only happen if the scene already exists.
                         * Find the scene using sld and set to datas. */
                        /* Set at the index of i. This way should the current
                         * SLD not be the first scene it won't fill the
                         * first slot in broadcastLookupDatas. This is important
                         * because the first slot is used for the single scene
                         * when using replace scenes. */
                        broadcastLookupData[i] = new(s);
                    }
                }

                /* Move identities
                 * to holder scene to preserve them.
                 * Required if a single scene is specified. Cannot rely on
                 * loadSingleScene since it is only true if the single scene
                 * must be loaded, which may be false if it's already loaded on
                 * the server. */
                // Do not run if running as client, and server is active. This would have already run as server.
                if (!asHost)
                {
                    Scene moveScene = GetMovedObjectsScene();
                    foreach (NetworkObject nob in sceneLoadData.MovedNetworkObjects)
                    {
                        // NetworkObject might be null if client lost observation of it.
                        if (nob != null && CanMoveNetworkObject(nob, true))
                            UnitySceneManager.MoveGameObjectToScene(nob.gameObject, moveScene);
                    }
                }

                // Connection scenes handles prior to ConnectionScenes being modified.
                List<int> connectionScenesHandlesCached = new();
                // If replacing scenes.
                if (replaceScenes != ReplaceOption.None)
                {
                    /* Resetting SceneConnections. */
                    /* If server and replacing scenes.
                     * It's important to run this AFTER moving MovedNetworkObjects
                     * so that they are no longer in the scenes they are leaving. Otherwise
                     * the scene condition would pick them up as still in the leaving scene. */
                    if (asServer)
                    {
                        Scene[] sceneConnectionsKeys = SceneConnections.Keys.ToArray();
                        for (int i = 0; i < sceneConnectionsKeys.Length; i++)
                            connectionScenesHandlesCached.Add(SceneHandleCompatibility.ToInt(sceneConnectionsKeys[i]));

                        // If global then remove all connections from all scenes.
                        if (data.ScopeType == SceneScopeType.Global)
                        {
                            foreach (Scene s in sceneConnectionsKeys)
                                RemoveAllConnectionsFromScene(s);
                        }
                        // Connections.
                        else if (data.ScopeType == SceneScopeType.Connections)
                        {
                            RemoveConnectionsFromNonGlobalScenes(data.Connections);
                        }
                    }
                    // As client set scenes id cache to local connection scenes.
                    else
                    {
                        foreach (Scene s in NetworkManager.ClientManager.Connection.Scenes)
                            connectionScenesHandlesCached.Add(SceneHandleCompatibility.ToInt(s));
                    }
                }


                /* Scene unloading if replacing scenes.
                 *
                 * Unload all scenes except MovedObjectsHolder. Also don't
                 * unload GlobalScenes if loading as connection. */
                List<Scene> unloadableScenes = new();
                // Do not run if running as client, and server is active. This would have already run as server.
                if (replaceScenes != ReplaceOption.None && !asHost)
                {
                    // See what scenes can be unloaded based on replace options.
                    for (int i = 0; i < UnitySceneManager.sceneCount; i++)
                    {
                        Scene s = UnitySceneManager.GetSceneAt(i);
                        // MovedObjectsScene will never be unloaded.
                        if (s == GetMovedObjectsScene())
                            continue;
                        /* Scene is in one of the scenes being loaded.
                         * This can occur when trying to load additional clients
                         * into an existing scene. */
                        if (requestedLoadSceneNames.Contains(s.name))
                            continue;
                        // Same as above but using handles.
                        if (requestedLoadSceneHandles.Contains(SceneHandleCompatibility.ToInt(s)))
                            continue;
                        /* Cannot unload global scenes. If
                         * replace scenes was used for a global
                         * load then global scenes would have been reset
                         * before this. */
                        if (IsGlobalScene(s))
                            continue;
                        // If scene must be manually unloaded then it cannot be unloaded here.
                        if (_manualUnloadScenes.Contains(s))
                            continue;

                        bool inScenesCache = connectionScenesHandlesCached.Contains(SceneHandleCompatibility.ToInt(s));
                        HashSet<NetworkConnection> conns;
                        bool inScenesCurrent = SceneConnections.ContainsKey(s);
                        // If was in scenes previously but isnt now then no connections reside in the scene.
                        if (inScenesCache && !inScenesCurrent)
                        {
                            // Intentionally left blank.
                        }
                        // If still in cache see if any connections exist.
                        else if (SceneConnections.TryGetValueIL2CPP(s, out conns))
                        {
                            // Still has clients in scene.
                            if (conns != null && conns.Count > 0)
                                continue;
                        }
                        // An offline scene.
                        else
                        {
                            // If not replacing all scenes then skip offline scenes.
                            if (replaceScenes != ReplaceOption.All)
                                continue;
                        }

                        unloadableScenes.Add(s);
                    }
                }

                /* Start event. */
                InvokeOnSceneLoadStart(data);
                if (unloadableScenes.Count > 0 || loadableScenes.Count > 0)
                    _sceneProcessor.LoadStart(data);
                // Unloaded scenes by name. Only used for information within callbacks.
                string[] unloadedNames = new string[unloadableScenes.Count];
                for (int i = 0; i < unloadableScenes.Count; i++)
                    unloadedNames[i] = unloadableScenes[i].name;
                /* Before unloading if !asServer and !asHost and replacing scenes
                 * then move all non scene networked objects to the moved
                 * objects holder. Otherwise network objects would get destroyed
                 * on the scene change and never respawned if server doesn't
                 * have a reason to update visibility. */
                if (!data.AsServer && !asHost && replaceScenes != ReplaceOption.None)
                {
                    // Only proceed if moving is disabled. This statement is nested for readability.
                    if (_moveClientObjects)
                    {
                        Scene s = GetMovedObjectsScene();
                        foreach (NetworkObject nob in NetworkManager.ClientManager.Objects.Spawned.Values)
                        {
                            if (CanMoveNetworkObject(nob, false))
                                UnitySceneManager.MoveGameObjectToScene(nob.gameObject, s);
                        }
                    }
                }
                /* Unloading scenes. */
                _sceneProcessor.UnloadStart(data);
                for (int i = 0; i < unloadableScenes.Count; i++)
                {
                    MoveClientHostObjects(unloadableScenes[i], asServer);
                    // Unload one at a time.
                    _sceneProcessor.BeginUnloadAsync(unloadableScenes[i]);
                    while (!_sceneProcessor.IsPercentComplete())
                        yield return null;
                }
                _sceneProcessor.UnloadEnd(data);

                // Scenes loaded.
                List<Scene> loadedScenes = new();
                /* Scene loading.
                /* Use additive to not thread lock server. */
                for (int i = 0; i < loadableScenes.Count; i++)
                {
                    // Start load async and wait for it to finish.
                    LoadSceneParameters loadSceneParameters = new()
                    {
                        loadSceneMode = LoadSceneMode.Additive,
                        localPhysicsMode = sceneLoadData.Options.LocalPhysics
                    };

                    /* How much percentage each scene load can be worth
                     * at maximum completion. EG: if there are two scenes
                     * 1f / 2f is 0.5f. */
                    float maximumIndexWorth = 1f / (float)loadableScenes.Count;

                    _sceneProcessor.BeginLoadAsync(loadableScenes[i].Name, loadSceneParameters);
                    while (!_sceneProcessor.IsPercentComplete())
                    {
                        float percent = _sceneProcessor.GetPercentComplete();
                        InvokePercentageChange(i, maximumIndexWorth, percent);
                        yield return null;
                    }

                    // Invokes OnScenePercentChange with progress.
                    void InvokePercentageChange(int index, float maximumWorth, float currentScenePercent)
                    {
                        /* Total percent will be how much percentage is complete
                         * in total. Initialize it with a value based on how many
                         * scenes are already fully loaded. */
                        float totalPercent = index * maximumWorth;
                        // Add this scenes progress onto total percent.
                        totalPercent += Mathf.Lerp(0f, maximumWorth, currentScenePercent);
                        // Dispatch with total percent.
                        InvokeOnScenePercentChange(data, totalPercent);
                    }

                    Scene lastLoadedScene = _sceneProcessor.GetLastLoadedScene();
                    /* If the lastLoadedScene returns default
                     * then the user is overriding the sceneprocessor
                     * and has not setup use for this particular API. */
                    if (lastLoadedScene == default)
                        lastLoadedScene = UnitySceneManager.GetSceneAt(UnitySceneManager.sceneCount - 1);

                    loadedScenes.Add(lastLoadedScene);
                    _sceneProcessor.AddLoadedScene(lastLoadedScene);
                }
                // When all scenes are loaded invoke with 100% done.
                InvokeOnScenePercentChange(data, 1f);

                /* Add to ManuallyUnloadScenes. */
                if (data.AsServer && !sceneLoadData.Options.AutomaticallyUnload)
                {
                    foreach (Scene s in loadedScenes)
                        _manualUnloadScenes.Add(s);
                }
                /* Move identities to first scene. */
                if (!asHost)
                {
                    // Find the first valid scene to move objects to.
                    Scene firstValidScene = default;
                    // If to stack scenes.
                    if (sceneLoadData.Options.AllowStacking)
                    {
                        Scene firstScene = sceneLoadData.GetFirstLookupScene();
                        /* If the first lookup data contains a handle and the scene
                         * is found for that handle then use that as the moved to scene.
                         * Nobs always move to the first specified scene. */
                        if (sceneLoadData.SceneLookupDatas[0].Handle != 0 && !string.IsNullOrEmpty(firstScene.name))
                        {
                            firstValidScene = firstScene;
                        }
                        // If handle is not specified then used the last scene that has the same name as the first lookupData.
                        else
                        {
                            Scene lastSameSceneName = default;
                            for (int i = 0; i < UnitySceneManager.sceneCount; i++)
                            {
                                Scene s = UnitySceneManager.GetSceneAt(i);
                                if (s.name == firstScene.name)
                                    lastSameSceneName = s;
                            }

                            /* Shouldn't be possible since the scene will always exist either by
                             * just being loaded or already loaded. */
                            if (string.IsNullOrEmpty(lastSameSceneName.name))
                                NetworkManager.LogError($"Scene {sceneLoadData.SceneLookupDatas[0].Name} could not be found in loaded scenes.");
                            else
                                firstValidScene = lastSameSceneName;
                        }
                    }
                    // Not stacking.
                    else
                    {
                        firstValidScene = sceneLoadData.GetFirstLookupScene();
                        // If not found by look then try firstloaded.
                        if (string.IsNullOrEmpty(firstValidScene.name))
                            firstValidScene = GetFirstLoadedScene();
                    }

                    // Gets first scene loaded this method call.
                    Scene GetFirstLoadedScene()
                    {
                        if (loadedScenes.Count > 0)
                            return loadedScenes[0];

                        return default;
                    }

                    // If firstValidScene is still invalid then throw.
                    if (string.IsNullOrEmpty(firstValidScene.name))
                    {
                        NetworkManager.LogError($"Unable to move objects to a new scene because new scene lookup has failed.");
                    }
                    // Move objects from movedobejctsscene to first valid scene.
                    else
                    {
                        Scene s = GetMovedObjectsScene();
                        s.GetRootGameObjects(_movingObjects);

                        foreach (GameObject go in _movingObjects)
                            UnitySceneManager.MoveGameObjectToScene(go, firstValidScene);
                    }
                }

                _sceneProcessor.ActivateLoadedScenes();
                // Wait until everything is loaded (done).
                yield return _sceneProcessor.AsyncsIsDone();
                _sceneProcessor.LoadEnd(data);

                /* Wait until loadedScenes are all marked as done.
                 * This is an extra precautionary step because on some devices
                 * the AsyncIsDone returns true before scenes are actually loaded. */
                bool allScenesLoaded;
                do
                {
                    // Reset state for iteration https://github.com/FirstGearGames/FishNet/issues/322
                    allScenesLoaded = true;
                    foreach (Scene s in loadedScenes)
                    {
                        if (!s.isLoaded)
                        {
                            allScenesLoaded = false;
                            break;
                        }
                    }
                    yield return null;
                } while (!allScenesLoaded);

                SetActiveScene_Local();

                void SetActiveScene_Local()
                {
                    bool byUser;
                    Scene preferredActiveScene = GetUserPreferredActiveScene(sceneLoadData.PreferredActiveScene, asServer, out byUser);
                    // If preferred still is not set then try to figure it out.
                    if (!preferredActiveScene.IsValid())
                    {
                        bool setToFirstLookup = false;
                        // If any scenes are being replaced see if active needs to be updated.
                        if (sceneLoadData.ReplaceScenes != ReplaceOption.None)
                        {
                            // If load is for a connection and server isnt started.
                            setToFirstLookup |= data.ScopeType == SceneScopeType.Connections && !NetworkManager.IsServerStarted;
                            /* If current active is the movedObjectsHolder, such as moved objects.
                             * This can happen when replacing a scene that was active and the next in line is
                             * set by unity as one of the temp scenes. */
                            Scene activeScene = UnitySceneManager.GetActiveScene();
                            setToFirstLookup |= activeScene == GetMovedObjectsScene();
                        }

                        if (setToFirstLookup)
                            preferredActiveScene = sceneLoadData.GetFirstLookupScene();
                    }

                    SetActiveScene(preferredActiveScene, asServer, byUser);
                }

                // Only the server needs to find scene handles to send to client. Client will send these back to the server.
                if (asServer)
                {
                    // Populate broadcastLookupDatas with any loaded scenes.
                    foreach (Scene s in loadedScenes)
                    {
                        SetInFirstNullIndex(s);

                        // Sets scene in the first null index of broadcastLookupDatas.
                        void SetInFirstNullIndex(Scene scene)
                        {
                            for (int i = 0; i < broadcastLookupData.Length; i++)
                            {
                                if (broadcastLookupData[i] == null)
                                {
                                    broadcastLookupData[i] = new(scene);
                                    return;
                                }
                            }

                            // If here there are no null entries.
                            NetworkManager.LogError($"Cannot add scene to broadcastLookupDatas, collection is full.");
                        }
                    }
                }

                /* If running as server and server is
                 * active then send scene changes to client.
                 * Making sure server is still active should it maybe
                 * have dropped during scene loading. */
                if (data.AsServer && NetworkManager.IsServerStarted)
                {
                    // Tell clients to load same scenes.
                    LoadScenesBroadcast msg = new()
                    {
                        QueueData = data
                    };
                    // Replace scene lookup datas with ones intended to broadcast to client.
                    msg.QueueData.SceneLoadData.SceneLookupDatas = broadcastLookupData;
                    // If networked scope then send to all.
                    if (data.ScopeType == SceneScopeType.Global)
                    {
                        NetworkConnection[] conns = _serverManager.Clients.Values.ToArray();

                        AddClientPendingLoads(conns);

                        _serverManager.Broadcast(msg, requireAuthenticated: true);
                    }
                    // If connections scope then only send to connections.
                    else if (data.ScopeType == SceneScopeType.Connections)
                    {
                        AddClientPendingLoads(data.Connections);

                        for (int i = 0; i < data.Connections.Length; i++)
                        {
                            NetworkConnection c = data.Connections[i];
                            if (c.IsValid() && c.IsAuthenticated)
                                data.Connections[i].Broadcast(msg, requireAuthenticated: true);
                        }
                    }

                    void AddClientPendingLoads(NetworkConnection[] lConns)
                    {
                        SceneLookupData[] slds = msg.QueueData.SceneLoadData.SceneLookupDatas;
                        foreach (SceneLookupData sld in slds)
                            AddPendingLoad(lConns, sld.Handle);
                    }
                }
                /* If running as client then send a message
                 * to the server to tell them the scene was loaded.
                 * This allows the server to add the client
                 * to the scene for checkers. */
                else if (!data.AsServer && NetworkManager.IsClientStarted)
                {
                    // Remove from old scenes.
                    foreach (Scene item in unloadableScenes)
                    {
                        if (item.IsValid())
                            localConnection.RemoveFromScene(item);
                    }
                    // Add local client to scenes.
                    foreach (Scene item in loadedScenes)
                        localConnection.AddToScene(item);

                    TryInvokeLoadedStartScenes(_clientManager.Connection, false);

                    ClientScenesLoadedBroadcast msg = new()
                    {
                        SceneLookupDatas = sceneLoadData.SceneLookupDatas
                    };

                    _clientManager.Broadcast(msg);
                }

                InvokeOnSceneLoadEnd(data, requestedLoadSceneNames, loadedScenes, unloadedNames);
            }
            finally
            {
                _serverGlobalScenesLoading.Clear();
            }
        }

        /// <summary>
        /// Received on client when connection scenes must be loaded.
        /// </summary>
        /// <param name = "conn"></param>
        /// <param name = "msg"></param>
        private void OnServerLoadedScenes(LoadScenesBroadcast msg, Channel channel)
        {
            // Null data is sent by the server when there are no start scenes to load.
            if (msg.QueueData == null)
            {
                TryInvokeLoadedStartScenes(_clientManager.Connection, false);
            }
            else
            {
                LoadQueueData qd = msg.QueueData;
                if (qd.ScopeType == SceneScopeType.Global)
                    LoadGlobalScenes_Internal(qd.SceneLoadData, qd.GlobalScenes, false);
                else
                    LoadConnectionScenes_Internal(Array.Empty<NetworkConnection>(), qd.SceneLoadData, qd.GlobalScenes, false);
            }
        }
        #endregion

        #region UnloadScenes.
        /// <summary>
        /// Unloads scenes on the server and for all clients.
        /// </summary>
        /// <param name = "sceneUnloadData">Data about which scenes to unload.</param>
        public void UnloadGlobalScenes(SceneUnloadData sceneUnloadData)
        {
            if (!CanExecute(true, true))
                return;

            UnloadGlobalScenes_Internal(sceneUnloadData, _globalScenes, true);
        }

        private void UnloadGlobalScenes_Internal(SceneUnloadData sceneUnloadData, string[] globalScenes, bool asServer)
        {
            UnloadQueueData uqd = new(SceneScopeType.Global, Array.Empty<NetworkConnection>(), sceneUnloadData, globalScenes, asServer);
            QueueOperation(uqd);
        }

        /// <summary>
        /// Unloads scenes on server and tells a connection to unload them as well. Other connections will not unload this scene.
        /// </summary>
        /// <param name = "connection">Connection to unload scenes for.</param>
        /// <param name = "sceneUnloadData">Data about which scenes to unload.</param>
        public void UnloadConnectionScenes(NetworkConnection connection, SceneUnloadData sceneUnloadData)
        {
            // This cannot use cache because the array will persist for many frames after this method completion.
            UnloadConnectionScenes(new NetworkConnection[] { connection }, sceneUnloadData);
        }

        /// <summary>
        /// Unloads scenes on server and tells connections to unload them as well. Other connections will not unload this scene.
        /// </summary>
        /// <param name = "connections">Connections to unload scenes for.</param>
        /// <param name = "sceneUnloadData">Data about which scenes to unload.</param>
        public void UnloadConnectionScenes(NetworkConnection[] connections, SceneUnloadData sceneUnloadData)
        {
            UnloadConnectionScenes_Internal(connections, sceneUnloadData, _globalScenes, true);
        }

        /// <summary>
        /// Unloads scenes on server without telling any connections to unload them.
        /// </summary>
        /// <param name = "sceneUnloadData">Data about which scenes to unload.</param>
        public void UnloadConnectionScenes(SceneUnloadData sceneUnloadData)
        {
            UnloadConnectionScenes_Internal(Array.Empty<NetworkConnection>(), sceneUnloadData, _globalScenes, true);
        }

        private void UnloadConnectionScenes_Internal(NetworkConnection[] connections, SceneUnloadData sceneUnloadData, string[] globalScenes, bool asServer)
        {
            if (!CanExecute(asServer, true))
                return;
            if (SceneDataInvalid(sceneUnloadData, true))
                return;

            UnloadQueueData uqd = new(SceneScopeType.Connections, connections, sceneUnloadData, globalScenes, asServer);
            QueueOperation(uqd);
        }

        /// <summary>
        /// Loads scenes within QueuedSceneLoads.
        /// </summary>
        /// <returns></returns>
        private IEnumerator __UnloadScenes()
        {
            UnloadQueueData data = _queuedOperations[0] as UnloadQueueData;
            SceneUnloadData sceneUnloadData = data.SceneUnloadData;

            // If connection went inactive.
            if (!ConnectionActive(data.AsServer))
                yield break;

            /* Some actions should not run as client if server is also active.
             * This is to keep things from running twice. */
            bool asClientHost = !data.AsServer && NetworkManager.IsServerStarted;
            ///True if running asServer.
            bool asServer = data.AsServer;

            // Get scenes to unload.
            Scene[] scenes = GetScenes(sceneUnloadData.SceneLookupDatas);
            /* No scenes found. Only run this if not asHost.
             * While asHost scenes will possibly not exist because
             * server side has already unloaded them. But rest of
             * the unload should continue. */
            if (scenes.Length == 0 && !asClientHost)
            {
                NetworkManager.LogWarning($"Scene lookup data of length {sceneUnloadData.SceneLookupDatas.Length} could not find any scenes to unload. This may occur when trying to unload a scene only by handle. Consider using the scene reference or handle and name while creating SceneLookupData.");
                yield break;
            }

            /* Remove from global scenes
             * if server and scope is global.
             * All passed in scenes should be removed from global
             * regardless of if they're valid or not. If they are invalid,
             * then they shouldn't be in global to begin with. */
            if (asServer && data.ScopeType == SceneScopeType.Global)
            {
                RemoveFromGlobalScenes(sceneUnloadData.SceneLookupDatas);
                // Update queue data.
                data.GlobalScenes = _globalScenes;
            }

            /* Remove connections. */
            if (asServer)
            {
                foreach (Scene s in scenes)
                {
                    // If global then remove all connections.
                    if (data.ScopeType == SceneScopeType.Global)
                        RemoveAllConnectionsFromScene(s);
                    // Connections.
                    else if (data.ScopeType == SceneScopeType.Connections)
                        RemoveConnectionsFromScene(data.Connections, s);
                }
            }


            /* This will contain all scenes which can be unloaded.
             * The collection will be modified through various checks. */
            List<Scene> unloadableScenes = scenes.ToList();
            /* Unloaded scenes manually created to overcome
             * the empty names in Scene structs after Unity unloads
             * a scene. */
            List<UnloadedScene> unloadedScenes = new();
            /* If asServer and KeepUnused then clear all unloadables.
             * The clients will still unload the scenes. */
            if ((asServer || asClientHost) && sceneUnloadData.Options.Mode == UnloadOptions.ServerUnloadMode.KeepUnused)
                unloadableScenes.Clear();
            // If clientOnly then force mode to unloadUnused.
            else if (!asServer && !asClientHost)
                sceneUnloadData.Options.Mode = UnloadOptions.ServerUnloadMode.UnloadUnused;
            /* Check to remove global scenes unloadableScenes.
             * This will need to be done if scenes are being unloaded
             * for connections. Global scenes cannot be unloaded as
             * connection. */
            if (data.ScopeType == SceneScopeType.Connections)
                RemoveGlobalScenes(unloadableScenes);
            // If set to unload unused only.
            if (sceneUnloadData.Options.Mode == UnloadOptions.ServerUnloadMode.UnloadUnused)
                RemoveOccupiedScenes(unloadableScenes);

            // If there are scenes to unload.
            if (unloadableScenes.Count > 0)
            {
                InvokeOnSceneUnloadStart(data);
                _sceneProcessor.UnloadStart(data);

                // Begin unloading.
                foreach (Scene s in unloadableScenes)
                {
                    if (!s.IsValid())
                    {
                        NetworkManager.LogWarning($"A scene was expected to be unloaded but could not due to it's referening going missing. This usually occurs when the same scene has been queued for unloading multiple times.");
                        continue;
                    }

                    unloadedScenes.Add(new(s));
                    MoveClientHostObjects(s, asServer);
                    /* Remove from manualUnloadedScenes.
                     * Scene may not be in this collection
                     * but removing is one call vs checking
                     * then removing. */
                    _manualUnloadScenes.Remove(s);

                    _sceneProcessor.BeginUnloadAsync(s);
                    while (!_sceneProcessor.IsPercentComplete())
                        yield return null;
                }

                _sceneProcessor.UnloadEnd(data);
            }

            /* Must yield after sceneProcessor handles things.
             * This is a Unity bug of sorts. I'm not entirely sure what
             * is happening, but without the yield it seems as though
             * the processor logic doesn't complete. This doesn't make much
             * sense given unity is supposed to be single threaded. Must be
             * something to do with the coroutine. */
            yield return null;

            bool byUser;
            Scene preferredActiveScene = GetUserPreferredActiveScene(sceneUnloadData.PreferredActiveScene, asServer, out byUser);
            SetActiveScene(preferredActiveScene, asServer, byUser);

            /* If running as server then make sure server
             * is still active after the unloads. If so
             * send out unloads to clients. */
            if (asServer && ConnectionActive(true))
            {
                // Tell clients to unload same scenes.
                UnloadScenesBroadcast msg = new()
                {
                    QueueData = data
                };
                // Global.
                if (data.ScopeType == SceneScopeType.Global)
                {
                    _serverManager.Broadcast(msg, true);
                }
                // Connections.
                else if (data.ScopeType == SceneScopeType.Connections)
                {
                    if (data.Connections != null)
                    {
                        for (int i = 0; i < data.Connections.Length; i++)
                        {
                            NetworkConnection conn = data.Connections[i];
                            // Would not be null from internals, but users might incorrectly pass null in.
                            if (conn.IsValid())
                                data.Connections[i].Broadcast(msg, true);
                        }
                    }
                }
            }
            else if (!asServer)
            {
                NetworkConnection localConnection = NetworkManager.ClientManager.Connection;
                // Remove from old scenes.
                foreach (Scene item in unloadableScenes)
                {
                    if (item.IsValid())
                        localConnection.RemoveFromScene(item);
                }
            }

            InvokeOnSceneUnloadEnd(data, unloadableScenes, unloadedScenes);
        }

        /// <summary>
        /// Received on clients when networked scenes must be unloaded.
        /// </summary>
        /// <param name = "conn"></param>
        /// <param name = "msg"></param>
        private void OnServerUnloadedScenes(UnloadScenesBroadcast msg, Channel channel)
        {
            UnloadQueueData qd = msg.QueueData;
            if (qd.ScopeType == SceneScopeType.Global)
                UnloadGlobalScenes_Internal(qd.SceneUnloadData, qd.GlobalScenes, false);
            else
                UnloadConnectionScenes_Internal(Array.Empty<NetworkConnection>(), qd.SceneUnloadData, qd.GlobalScenes, false);
        }
        #endregion

        /// <summary>
        /// Move objects visible to clientHost that are within an unloading scene.This ensures the objects are despawned on the client side rather than when the scene is destroyed.
        /// </summary>
        /// <param name = "scene"></param>
        private void MoveClientHostObjects(Scene scene, bool asServer)
        {
            if (!_moveClientObjects)
                return;
            /* The asServer isn't really needed. I could only call
             * this method when asServer is true. But for the sake
             * of preventing user-error (me being the user this time)
             * I've included it into the parameters. */
            if (!asServer)
                return;
            // Don't need to perform if not host.
            if (!NetworkManager.IsClientStarted)
                return;

            NetworkConnection clientConn = NetworkManager.ClientManager.Connection;
            /* It would be nice to see if the client wasn't even in the scene
             * here using SceneConnections but it's possible that the scene had been
             * wiped from SceneConnections earlier depending on how scenes are
             * loaded or unloaded. Instead we must iterate through spawned objects. */

            List<NetworkObject> movingNobs = CollectionCaches<NetworkObject>.RetrieveList();
            /* Rather than a get all networkobjects in scene
             * let's iterate the spawned objects instead. I imagine
             * in most scenarios iterating spawned would be faster.
             * That's a long one! */
            foreach (NetworkObject nob in NetworkManager.ServerManager.Objects.Spawned.Values)
            {
                // Not in the scene being destroyed.
                if (nob.gameObject.scene != scene)
                    continue;
                // ClientHost doesn't have visibility.
                if (!nob.Observers.Contains(clientConn))
                    continue;
                // Cannot move if not root.
                if (nob.transform.root != null)
                    continue;

                /* If here nob is in the same being
                 * destroyed and clientHost has visiblity. */
                movingNobs.Add(nob);
            }

            int count = movingNobs.Count;
            if (count > 0)
            {
                Scene moveScene = GetDelayedDestroyScene();
                for (int i = 0; i < count; i++)
                {
                    NetworkObject nob = movingNobs[i];
                    /* Force as not a scene object
                     * so that it becomes destroyed
                     * rather than disabled. */
                    nob.ClearRuntimeSceneObject();
                    /* If the object is already being despawned then
                     *just disable and move it. Otherwise despawn it
                     * on the server then move it. */
                    // Not deinitializing, despawn it then.
                    if (!nob.IsDeinitializing)
                        nob.Despawn();
                    else
                        nob.gameObject.SetActive(false);

                    UnitySceneManager.MoveGameObjectToScene(nob.gameObject, moveScene);
                }
            }
            CollectionCaches<NetworkObject>.Store(movingNobs);
        }

        /// <summary>
        /// Returns if a connection is in a scene using SceneConnections.
        /// </summary>
        /// <param name = "conn"></param>
        /// <param name = "scene"></param>
        /// <returns></returns>
        internal bool InSceneConnections(NetworkConnection conn, Scene scene)
        {
            if (!SceneConnections.TryGetValueIL2CPP(scene, out HashSet<NetworkConnection> hs))
                return false;
            else
                return hs.Contains(conn);
        }

        /// <summary>
        /// Adds the owner of nob to the gameObjects scene if there are no global scenes.
        /// </summary>
        public void AddOwnerToDefaultScene(NetworkObject nob)
        {
            // No owner.
            if (!nob.Owner.IsValid)
            {
                NetworkManager.LogWarning($"NetworkObject {nob.name} does not have an owner.");
                return;
            }
            // Won't add to default if there are globals.
            if (_globalScenes.Length > 0)
                return;

            AddConnectionToScene(nob.Owner, nob.gameObject.scene);
        }

        /// <summary>
        /// Adds a connection to a scene. This will always be called one connection at a time because connections are only added after they invidually validate loading the scene.
        /// Exposed for power users, use caution.
        /// </summary>
        /// <param name = "conn">Connection to add.</param>
        /// <param name = "scene">Scene to add the connection to.</param>
        public void AddConnectionToScene(NetworkConnection conn, Scene scene)
        {
            if (!conn.IsValid())
                return;
            if (!scene.isLoaded || !scene.IsValid())
            {
                NetworkManager.LogError($"Only valid, loaded scenes may be used.");
                return;
            }

            HashSet<NetworkConnection> hs;
            // Scene doesn't have any connections yet.
            bool inSceneConnections = SceneConnections.TryGetValueIL2CPP(scene, out hs);
            if (!inSceneConnections)
                hs = new();

            bool added = hs.Add(conn);
            if (added)
            {
                conn.AddToScene(scene);

                // If not yet added to scene connections.
                if (!inSceneConnections)
                    SceneConnections[scene] = hs;

                NetworkConnection[] arrayConn = new NetworkConnection[] { conn };
                InvokeClientPresenceChange(scene, arrayConn, true, true);
                RebuildObservers(arrayConn);
                InvokeClientPresenceChange(scene, arrayConn, true, false);

                /* Also need to rebuild all networkobjects
                 * for connection so other players can
                 * see them. */
                RebuildObservers(conn.Objects.ToArray());
            }
        }

        /// <summary>
        /// Removes connections from any scene which is not global.
        /// Exposed for power users, use caution.
        /// </summary>
        /// <param name = "conns"></param>
        public void RemoveConnectionsFromNonGlobalScenes(NetworkConnection[] conns)
        {
            List<Scene> removedScenes = new();

            foreach (KeyValuePair<Scene, HashSet<NetworkConnection>> item in SceneConnections)
            {
                Scene scene = item.Key;
                // Cannot remove from globla scenes.
                if (IsGlobalScene(scene))
                    continue;

                HashSet<NetworkConnection> hs = item.Value;
                List<NetworkConnection> connectionsRemoved = new();
                // Remove every connection from the scene.
                foreach (NetworkConnection c in conns)
                {
                    if (!c.IsValid())
                        continue;

                    bool removed = hs.Remove(c);
                    if (removed)
                    {
                        c.RemoveFromScene(scene);
                        connectionsRemoved.Add(c);
                    }
                }

                // If hashset is empty then remove scene from SceneConnections.
                if (hs.Count == 0)
                    removedScenes.Add(scene);

                if (connectionsRemoved.Count > 0)
                {
                    InvokeClientPresenceChange(scene, connectionsRemoved, false, true);
                    RebuildObservers(connectionsRemoved);
                    InvokeClientPresenceChange(scene, connectionsRemoved, false, false);
                }
            }

            foreach (Scene s in removedScenes)
                SceneConnections.Remove(s);

            /* Also rebuild observers for objects owned by connection.
             * This ensures other connections will lose visibility if
             * they no longer share a scene. */
            foreach (NetworkConnection c in conns)
                RebuildObservers(c.Objects.ToArray());
        }

        /// <summary>
        /// Removes connections from specified scenes.
        /// Exposed for power users, use caution.
        /// </summary>
        /// <param name = "conns">Connections to remove.</param>
        /// <param name = "scene">Scene to remove from.</param>
        public void RemoveConnectionsFromScene(NetworkConnection[] conns, Scene scene)
        {
            HashSet<NetworkConnection> hs;
            // No hashset for scene, so no connections are in scene.
            if (!SceneConnections.TryGetValueIL2CPP(scene, out hs))
                return;

            List<NetworkConnection> connectionsRemoved = new();
            // Remove every connection from the scene.
            foreach (NetworkConnection c in conns)
            {
                if (!c.IsValid())
                    continue;

                bool removed = hs.Remove(c);
                if (removed)
                {
                    c.RemoveFromScene(scene);
                    connectionsRemoved.Add(c);
                }
            }

            // If hashset is empty then remove scene from SceneConnections.
            if (hs.Count == 0)
                SceneConnections.Remove(scene);

            if (connectionsRemoved.Count > 0)
            {
                NetworkConnection[] connectionsRemovedArray = connectionsRemoved.ToArray();
                InvokeClientPresenceChange(scene, connectionsRemovedArray, false, true);
                RebuildObservers(connectionsRemovedArray);
                InvokeClientPresenceChange(scene, connectionsRemovedArray, false, false);
            }

            /* Also rebuild observers for objects owned by connection.
             * This ensures other connections will lose visibility if
             * they no longer share a scene. */
            foreach (NetworkConnection c in conns)
                RebuildObservers(c.Objects.ToArray());
        }

        /// <summary>
        /// Removes all connections from a scene.
        /// </summary>
        /// <param name = "scene">Scene to remove connections from.</param>
        public void RemoveAllConnectionsFromScene(Scene scene)
        {
            HashSet<NetworkConnection> hs;
            // No hashset for scene, so no connections are in scene.
            if (!SceneConnections.TryGetValueIL2CPP(scene, out hs))
                return;

            // On each connection remove them from specified scene.
            foreach (NetworkConnection c in hs)
                c.RemoveFromScene(scene);
            // Make hashset into list for presence change.
            NetworkConnection[] connectionsRemoved = hs.ToArray();

            // Clear hashset and remove entry from sceneconnections.
            hs.Clear();
            SceneConnections.Remove(scene);

            if (connectionsRemoved.Length > 0)
            {
                InvokeClientPresenceChange(scene, connectionsRemoved, false, true);
                RebuildObservers(connectionsRemoved);
                InvokeClientPresenceChange(scene, connectionsRemoved, false, false);
            }

            /* Also rebuild observers for objects owned by connection.
             * This ensures other connections will lose visibility if
             * they no longer share a scene. */
            foreach (NetworkConnection c in connectionsRemoved)
                RebuildObservers(c.Objects.ToArray());
        }

        #region Can Load/Unload Scene.
        /// <summary>
        /// Returns if a scene can be loaded locally.
        /// </summary>
        /// <returns></returns>
        private bool CanLoadScene(LoadQueueData qd, SceneLookupData sld)
        {
            bool foundByHandle;
            Scene s = sld.GetScene(out foundByHandle);
            // Try to find if scene is already loaded.
            bool alreadyLoaded = !string.IsNullOrEmpty(s.name);

            if (alreadyLoaded)
            {
                // Only servers can load the same scene multiple times for stacking.
                if (!qd.AsServer)
                    return false;
                // If can only load scenes which aren't loaded yet and scene is already loaded.
                if (!qd.SceneLoadData.Options.AllowStacking)
                    return false;
                /* Found by handle, this means the user is trying to specify
                 * exactly which scene to load into. When a handle is specified
                 * new instances will not be created, so a new scene cannot
                 * be loaded. */
                if (alreadyLoaded && foundByHandle)
                    return false;
            }

            // Fall through.
            return true;
        }
        #endregion

        #region Helpers.
        /// <summary>
        /// Rebuilds observers for networkObjects.
        /// </summary>
        /// <param name = "networkObjects"></param>
        private void RebuildObservers(IList<NetworkObject> networkObjects)
        {
            NetworkObject nob;
            int count = networkObjects.Count;
            for (int i = 0; i < count; i++)
            {
                nob = networkObjects[i];
                if (nob != null && nob.IsSpawned)
                    _serverManager.Objects.RebuildObservers(nob);
            }
        }

        /// <summary>
        /// Rebuilds all NetworkObjects for connection.
        /// </summary>
        internal void RebuildObservers(NetworkConnection connection)
        {
            List<NetworkConnection> connCache = CollectionCaches<NetworkConnection>.RetrieveList(connection);
            RebuildObservers(connCache);
            CollectionCaches<NetworkConnection>.Store(connCache);
        }

        /// <summary>
        /// Rebuilds all NetworkObjects for connections.
        /// </summary>
        internal void RebuildObservers(IList<NetworkConnection> connections)
        {
            int count = connections.Count;
            for (int i = 0; i < count; i++)
                _serverManager.Objects.RebuildObservers(connections[i]);
        }

        /// <summary>
        /// Invokes OnClientPresenceChange start or end.
        /// </summary>
        private void InvokeClientPresenceChange(Scene scene, IList<NetworkConnection> conns, bool added, bool start)
        {
            NetworkConnection c;
            int count = conns.Count;
            for (int i = 0; i < count; i++)
            {
                c = conns[i];
                ClientPresenceChangeEventArgs cpc = new(scene, c, added);
                if (start)
                    OnClientPresenceChangeStart?.Invoke(cpc);
                else
                    OnClientPresenceChangeEnd?.Invoke(cpc);
            }
        }
        #endregion

        #region GetScene.
        /// <summary>
        /// Gets scenes from SceneLookupData.
        /// </summary>
        /// <param name = "datas"></param>
        /// <returns></returns>
        private Scene[] GetScenes(SceneLookupData[] datas)
        {
            List<Scene> result = new();
            foreach (SceneLookupData sld in datas)
            {
                Scene s = sld.GetScene(out _);
                if (!string.IsNullOrEmpty(s.name))
                {
                    result.Add(s);
                }
            }

            return result.ToArray();
        }

        /// <summary>
        /// Returns a scene by name.
        /// </summary>
        /// <param name = "sceneName">Name of scene to retrieve.</param>
        /// <param name = "nm">NetworkManager to use for debug print. This value may be left null.</param>
        /// <param name = "warnIfDuplicates">True to warn if scene name is found loaded multiple times.</param>
        /// <returns></returns>
        public static Scene GetScene(string sceneName, NetworkManager nm = null, bool warnIfDuplicates = true)
        {
            Scene result = default;
            sceneName = sceneName.ToLower();

            int count = UnitySceneManager.sceneCount;
            for (int i = 0; i < count; i++)
            {
                Scene s = UnitySceneManager.GetSceneAt(i);
                // Matches.
                if (s.name.ToLower() == sceneName)
                {
                    // If result is already set.
                    if (result.IsValid())
                    {
                        if (warnIfDuplicates)
                        {
                            string msg = $"Scene name {s.name} is loaded multiple times. The first scene found will be returned. If you wish to unload multiple instances of a scene with the same name create {nameof(SceneLookupData)} using scene handles instead of name.";
                            nm.LogWarning(msg);
                            // No need to spam the message, break on first duplicate.
                            break;
                        }
                    }
                    else
                    {
                        result = s;
                    }
                }
            }

            return result;
        }

        /// <summary>
        /// Returns a scene by handle.
        /// </summary>
        /// <param name = "sceneHandle"></param>
        /// <returns></returns>
        public static Scene GetScene(int sceneHandle)
        {
            int count = UnitySceneManager.sceneCount;
            for (int i = 0; i < count; i++)
            {
                Scene s = UnitySceneManager.GetSceneAt(i);
                if (SceneHandleCompatibility.Equals(s, sceneHandle))
                    return s;
            }

            return new();
        }
        #endregion

        /// <summary>
        /// Returns if GlobalScenes contains scene.
        /// </summary>
        /// <param name = "s"></param>
        /// <returns></returns>
        private bool IsGlobalScene(Scene scene)
        {
            foreach (string item in _globalScenes)
            {
                string nameOnly = System.IO.Path.GetFileNameWithoutExtension(item);
                if (item == scene.name || nameOnly == scene.name)
                    return true;
            }
            return false;
        }

        /// <summary>
        /// Warns if any scene names in GlobalScenes are unsupported.
        /// This only applies to FishNet version 3.
        /// </summary>
        private void CheckForDuplicateGlobalSceneNames()
        {
            /* This is being removed between version 4.0.0 to 4.1.0 */
            HashSet<string> namesOnly = CollectionCaches<string>.RetrieveHashSet();
            foreach (string item in _globalScenes)
            {
                string name = System.IO.Path.GetFileNameWithoutExtension(item);
                if (namesOnly.Contains(name))
                {
                    NetworkManager.LogWarning($"There are multiple global scenes loaded with the same NameOnly. This occurs when a global scene has the same name as another but resides in a different folder path. Each global scene name must be unique.");
                    break;
                }
                else
                {
                    namesOnly.Add(name);
                }
            }
        }

        /// <summary>
        /// Removes datas from GlobalScenes.
        /// </summary>
        /// <param name = "scenes"></param>
        private void RemoveFromGlobalScenes(Scene scene)
        {
            RemoveFromGlobalScenes(new SceneLookupData[] { SceneLookupData.CreateData(scene) });
        }

        /// <summary>
        /// Removes datas from GlobalScenes.
        /// </summary>
        /// <param name = "scenes"></param>
        private void RemoveFromGlobalScenes(SceneLookupData[] datas)
        {
            List<string> newGlobalScenes = _globalScenes.ToList();
            int startCount = newGlobalScenes.Count;
            // Remove scenes.
            for (int i = 0; i < datas.Length; i++)
                newGlobalScenes.Remove(datas[i].Name);

            // If any were removed remake globalscenes.
            if (startCount != newGlobalScenes.Count)
                _globalScenes = newGlobalScenes.ToArray();
        }

        /// <summary>
        /// Removes GlobalScenes from scenes.
        /// </summary>
        /// <param name = "scenes"></param>
        /// <returns></returns>
        private void RemoveGlobalScenes(List<Scene> scenes)
        {
            for (int i = 0; i < scenes.Count; i++)
            {
                foreach (string gs in _globalScenes)
                {
                    if (gs == scenes[i].name)
                    {
                        scenes.RemoveAt(i);
                        i--;
                    }
                }
            }
        }

        /// <summary>
        /// Removes occupied scenes from scenes.
        /// </summary>
        /// <param name = "scenes"></param>
        private void RemoveOccupiedScenes(List<Scene> scenes)
        {
            for (int i = 0; i < scenes.Count; i++)
            {
                Scene s = scenes[i];

                if (SceneConnections.TryGetValueIL2CPP(s, out _) || _pendingClientSceneLoads.HasSceneAnyPendingLoads(SceneHandleCompatibility.ToInt(s)))
                {
                    scenes.RemoveAt(i);
                    i--;
                }
            }
        }

        /// <summary>
        /// Adds a pending load for a connection.
        /// </summary>
        private void AddPendingLoad(NetworkConnection[] conns, int sceneHandle)
        {
            foreach (NetworkConnection c in conns)
            {
                /* Make sure connection is active. This should always be true
                 * but perhaps disconnect happened as scene was loading on server
                 * therefor it cannot be sent to the client.
                 * Also only authenticated clients can load scenes. */
                if (!c.IsActive || !c.IsAuthenticated)
                    continue;

                _pendingClientSceneLoads.AddClientToScene(c, sceneHandle);
            }
        }

        /// <summary>
        /// Sets the first global scene as the active scene.
        /// If a global scene is not available then FallbackActiveScene is used.
        /// </summary>
        private void SetActiveScene(Scene preferredScene, bool asServer, bool byUser)
        {
            // If user specified then skip figuring it out checks.
            if (byUser && preferredScene.IsValid())
            {
                CompleteSetActive(preferredScene);
            }
            // Setting active scene is not used.
            else if (!_setActiveScene)
            {
                // Still invoke event with current scene.
                Scene s = UnitySceneManager.GetActiveScene();
                CompleteSetActive(s);
            }
            // Need to figure out which scene to use.
            else
            {
                Scene s = default;

                if (_globalScenes.Length > 0)
                    s = GetScene(_globalScenes[0], NetworkManager, false);
                else if (preferredScene.IsValid())
                    s = preferredScene;

                /* If scene isn't set from global then make
                 * sure currently active isn't the movedobjectscene.
                 * If it is, then use the fallback scene. */
                if (string.IsNullOrEmpty(s.name) && UnitySceneManager.GetActiveScene() == _movedObjectsScene)
                    s = GetFallbackActiveScene();

                CompleteSetActive(s);
            }

            // Completes setting the active scene with specified value.
            void CompleteSetActive(Scene scene)
            {
                bool sceneValid = scene.IsValid();
                if (sceneValid)
                    UnitySceneManager.SetActiveScene(scene);

                OnActiveSceneSet?.Invoke(byUser);
                OnActiveSceneSetInternal?.Invoke(asServer);

                if (sceneValid)
                {
                    // Also update light probes.
                    if (_lightProbeUpdating == LightProbeUpdateType.Asynchronous)
                        LightProbes.TetrahedralizeAsync();
                    else if (_lightProbeUpdating == LightProbeUpdateType.BlockThread)
                        LightProbes.Tetrahedralize();
                }
            }
        }

        /// <summary>
        /// Returns the FallbackActiveScene.
        /// </summary>
        /// <returns></returns>
        private Scene GetFallbackActiveScene() => _sceneProcessor.GetFallbackActiveScene();

        /// <summary>
        /// Returns the MovedObjectsScene.
        /// </summary>
        /// <returns></returns>
        private Scene GetMovedObjectsScene() => _sceneProcessor.GetMovedObjectsScene();

        /// <summary>
        /// Returns the DelayedDestroyScene.
        /// </summary>
        /// <returns></returns>
        private Scene GetDelayedDestroyScene() => _sceneProcessor.GetDelayedDestroyScene();

        /// <summary>
        /// Returns a preferred active scene to use.
        /// </summary>
        private Scene GetUserPreferredActiveScene(PreferredScene ps, bool asServer, out bool byUser)
        {
            byUser = false;
            SceneLookupData sld = asServer ? ps.Server : ps.Client;
            // Not specified.
            if (sld == null)
                return default;

            Scene s = sld.GetScene(out _);
            if (s.IsValid())
                byUser = true;
            return s;
        }

        #region Sanity checks.
        /// <summary>
        /// Returns if iterating queue.
        /// True will be returned even if not iterating queue if the iteration had completed with the time requirement.
        internal bool IsIteratingQueue(float completionTimeRequirement = 0f)
        {
            return IteratingQueue || Time.unscaledTime - QueueCompleteTime < completionTimeRequirement;
        }

        /// <summary>
        /// Returns if a SceneLoadData is valid.
        /// </summary>
        /// <param name = "data"></param>
        /// <param name = "error"></param>
        /// <returns></returns>
        private bool SceneDataInvalid(SceneLoadData data, bool error)
        {
            bool result = data.DataInvalid();
            if (result && error)
                NetworkManager.LogError(INVALID_SCENELOADDATA);

            return result;
        }

        /// <summary>
        /// Returns if a SceneLoadData is valid.
        /// </summary>
        /// <param name = "data"></param>
        /// <param name = "error"></param>
        /// <returns></returns>
        private bool SceneDataInvalid(SceneUnloadData data, bool error)
        {
            bool result = data.DataInvalid();
            if (result && error)
                NetworkManager.LogError(INVALID_SCENEUNLOADDATA);


            return result;
        }

        /// <summary>
        /// Returns if connection is active for server or client in association with AsServer.
        /// </summary>
        /// <param name = "asServer"></param>
        /// <returns></returns>
        private bool ConnectionActive(bool asServer)
        {
            return asServer ? NetworkManager.IsServerStarted : NetworkManager.IsClientStarted;
        }

        /// <summary>
        /// Returns if a method can execute.
        /// </summary>
        /// <param name = "asServer"></param>
        /// <param name = "warn"></param>
        /// <returns></returns>
        private bool CanExecute(bool asServer, bool warn)
        {
            bool result;
            if (asServer)
            {
                result = NetworkManager.IsServerStarted;
                if (!result && warn)
                    NetworkManager.LogWarning($"Method cannot be called as the server is not active.");
            }
            else
            {
                result = NetworkManager.IsClientStarted;
                if (!result && warn)
                    NetworkManager.LogWarning($"Method cannot be called as the client is not active.");
            }

            return result;
        }
        #endregion
    }
}

// #endif

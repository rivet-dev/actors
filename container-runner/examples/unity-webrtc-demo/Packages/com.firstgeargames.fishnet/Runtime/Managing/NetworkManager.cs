#if UNITY_EDITOR || DEVELOPMENT_BUILD
#define DEVELOPMENT
#endif
#if UNITY_EDITOR
using FishNet.Editing.PrefabCollectionGenerator;
using UnityEditor;
#endif
using FishNet.Connection;
using FishNet.Managing.Client;
using FishNet.Managing.Server;
using FishNet.Managing.Timing;
using FishNet.Managing.Transporting;
using UnityEngine;
using FishNet.Managing.Scened;
using FishNet.Object;
using FishNet.Documenting;
using System.Collections.Generic;
using System;
using FishNet.Managing.Observing;
using System.Linq;
using FishNet.Broadcast;
using FishNet.Managing.Debugging;
using FishNet.Managing.Object;
using FishNet.Transporting;
using FishNet.Managing.Statistic;
using FishNet.Utility.Performance;
using FishNet.Component.ColliderRollback;
using FishNet.Component.Transforming.Beta;
using FishNet.Configuring;
using FishNet.Configuring.EditorCloning;
using FishNet.Managing.Predicting;
using GameKit.Dependencies.Utilities;

namespace FishNet.Managing
{
    /// <summary>
    /// Acts as a container for all things related to your networking session.
    /// </summary>
    [DefaultExecutionOrder(short.MinValue)]
    [DisallowMultipleComponent]
    [AddComponentMenu("FishNet/Manager/NetworkManager")]
    public sealed partial class NetworkManager : MonoBehaviour
    {
        #region Types.
        /// <summary>
        /// How to persist with multiple NetworkManagers.
        /// </summary>
        public enum PersistenceType
        {
            /// <summary>
            /// Destroy any new NetworkManagers.
            /// </summary>
            DestroyNewest,
            /// <summary>
            /// Destroy previous NetworkManager when a new NetworkManager occurs.
            /// </summary>
            DestroyOldest,
            /// <summary>
            /// Allow multiple NetworkManagers, do not destroy any automatically.
            /// </summary>
            AllowMultiple
        }
        #endregion

        #region Public.
        /// <summary>
        /// True if this instance of the NetworkManager is initialized.
        /// </summary>
        public bool Initialized { get; private set; }
        /// <summary>
        /// </summary>
        private static List<NetworkManager> _instances = new();
        /// <summary>
        /// Currently initialized NetworkManagers.
        /// </summary>
        public static IReadOnlyList<NetworkManager> Instances
        {
            get
            {
                /* Remove null instances of NetworkManager.
                 * This shouldn't happen because instances are removed
                 * OnDestroy but none the less something is causing
                 * it. */
                for (int i = 0; i < _instances.Count; i++)
                {
                    if (_instances[i] == null)
                    {
                        _instances.RemoveAt(i);
                        i--;
                    }
                }

                return _instances;
            }
        }
        /// <summary>
        /// PredictionManager for this NetworkManager.
        /// </summary>
        internal PredictionManager PredictionManager { get; private set; }
        /// <summary>
        /// ServerManager for this NetworkManager.
        /// </summary>
        public ServerManager ServerManager { get; private set; }
        /// <summary>
        /// ClientManager for this NetworkManager.
        /// </summary>
        public ClientManager ClientManager { get; private set; }
        /// <summary>
        /// TransportManager for this NetworkManager.
        /// </summary>
        public TransportManager TransportManager { get; private set; }
        /// <summary>
        /// TimeManager for this NetworkManager.
        /// </summary>
        public TimeManager TimeManager { get; private set; }
        /// <summary>
        /// SceneManager for this NetworkManager.
        /// </summary>
        public SceneManager SceneManager { get; private set; }
        /// <summary>
        /// ObserverManager for this NetworkManager.
        /// </summary>
        public ObserverManager ObserverManager { get; private set; }
        #if FISHNET_THREADED_TICKSMOOTHERS
        /// <summary>
        /// TickSmoothingManager for this NetworkManager.
        /// </summary>
        public TickSmoothingManager TickSmoothingManager { get; private set; }
        #endif
        /// <summary>
        /// DebugManager for this NetworkManager.
        /// </summary>
        public DebugManager DebugManager { get; private set; }
        /// <summary>
        /// StatisticsManager for this NetworkManager.
        /// </summary>
        public StatisticsManager StatisticsManager { get; private set; }
        /// <summary>
        /// An empty connection reference. Used when a connection cannot be found to prevent object creation.
        /// </summary>
        [APIExclude]
        public static NetworkConnection EmptyConnection { get; private set; } = new();
        #endregion

        #region Internal.
        #if DEVELOPMENT && !UNITY_SERVER
        /// <summary>
        /// Names of broadcasts.
        /// Key: Broadcast key.
        /// Value: Name.
        /// </summary>
        /// <remarks>Only broadcast which were registered at least once will be found within this collection. This collection is never cleared.</remarks>
        private Dictionary<ushort, string> _broadcastNames = new();

        /// <summary>
        /// Gets a broadcast name by key.
        /// </summary>
        internal string GetBroadcastName(ushort key)
        {
            if (_broadcastNames.TryGetValueIL2CPP(key, out string result))
                return result;

            const string notFoundName = "Unregistered Broadcasts";

            return notFoundName;
        }

        /// <summary>
        /// Sets a broadcast name to be used with GetBroadcastName.
        /// </summary>
        internal void SetBroadcastName<T>(ushort key) where T : struct, IBroadcast
        {
            // Avoid allocation by checking if key already exist.
            if (!_broadcastNames.ContainsKey(key))
                _broadcastNames[key] = typeof(T).Name;
        }
        #endif
        /// <summary>
        /// Starting index for RpcLinks.
        /// </summary>
        internal static ushort StartingRpcLinkIndex;
        #if DEVELOPMENT
        /// <summary>
        /// Logs data about parser to help debug.
        /// </summary>
        internal PacketIdHistory PacketIdHistory = new();
        #endif
        /// <summary>
        /// Timestamp when the first NetworkManager instance was launched.
        /// </summary>
        internal static long LaunchTimestamp;
        #endregion

        #region Serialized.
        #if UNITY_EDITOR
        /// <summary>
        /// True to refresh the DefaultPrefabObjects collection whenever the editor enters play mode. This is an attempt to alleviate the DefaultPrefabObjects scriptable object not refreshing when using multiple editor applications such as ParrelSync.
        /// </summary>
        [Tooltip("True to refresh the DefaultPrefabObjects collection whenever the editor enters play mode. This is an attempt to alleviate the DefaultPrefabObjects scriptable object not refreshing when using multiple editor applications such as ParrelSync.")]
        [SerializeField]
        private bool _refreshDefaultPrefabs = false;
        #endif
        /// <summary>
        /// True to have your application run while in the background.
        /// </summary>
        [Tooltip("True to have your application run while in the background.")]
        [SerializeField]
        private bool _runInBackground = true;
        /// <summary>
        /// True to make this instance DontDestroyOnLoad. This is typical if you only want one NetworkManager.
        /// </summary>
        [Tooltip("True to make this instance DontDestroyOnLoad. This is typical if you only want one NetworkManager.")]
        [SerializeField]
        private bool _dontDestroyOnLoad = true;
        /// <summary>
        /// Object pool to use for this NetworkManager. Value may be null.
        /// </summary>
        public ObjectPool ObjectPool => _objectPool;
        [Tooltip("Object pool to use for this NetworkManager. Value may be null.")]
        [SerializeField]
        private ObjectPool _objectPool;
        /// <summary>
        /// How to persist when other NetworkManagers are introduced.
        /// </summary>
        [Tooltip("How to persist when other NetworkManagers are introduced.")]
        [SerializeField]
        private PersistenceType _persistence = PersistenceType.DestroyNewest;
        #endregion

        #region Private.
        /// <summary>
        /// Value of Application.RunInBackground before starting any network connection.
        /// </summary>
        /// <remarks>A null value indicates not yet set.</remarks>
        private bool? _offlineApplicationRunInBackground;
        #endregion

        #region Const.
        /// <summary>
        /// Version of this release.
        /// </summary>
        public const string FISHNET_VERSION = "4.7.2";
        /// <summary>
        /// Maximum framerate allowed.
        /// </summary>
        internal const ushort MAXIMUM_FRAMERATE = 500;
        /// <summary>
        /// Timestamp to use when value is not set.
        /// </summary>
        internal const long UNSET_LAUNCH_TIMESTAMP = 0;
        /// <summary>
        /// Value to use when the launch timestamp is calculated, but happens to be the unset value.
        /// </summary>
        private const long REPAIR_LAUNCH_TIMESTAMP_CONFLICT_VALUE = UNSET_LAUNCH_TIMESTAMP + 1;
        #endregion

        private void Awake()
        {
            InitializeLogging();
            if (!ValidateSpawnablePrefabs(true))
                return;

            if (StartingRpcLinkIndex == 0)
                StartingRpcLinkIndex = (ushort)(Enums.GetHighestValue<PacketId>() + 1);

            if (!CanPersist())
                return;

            // If is the first instance then set launch timestamp.
            if (_instances.Count == 0)
            {
                LaunchTimestamp = DateTime.Now.ToBinary();
                // What are the odds fo this happening!
                if (LaunchTimestamp == UNSET_LAUNCH_TIMESTAMP)
                    LaunchTimestamp = REPAIR_LAUNCH_TIMESTAMP_CONFLICT_VALUE;
            }

            bool isDefaultPrefabs = SpawnablePrefabs != null && SpawnablePrefabs is DefaultPrefabObjects;
            CloneChecker.IsMultiplayerClone(out EditorCloneType cloneType);

            #if UNITY_EDITOR
            /* If first instance then force
             * default prefabs to repopulate.
             * This is only done in editor because
             * cloning tools sometimes don't synchronize
             * scriptable object changes, which is what
             * the default prefabs is. */

            /* There is an issue with Unity Multiplayer where if a prefab
             * is modified in the main editor with a multiplayer window open,
             * the modified prefab becomes null in the collection. Because of this,
             * we must re-generate on the cloned window. */
            if (isDefaultPrefabs && (cloneType == EditorCloneType.UnityMultiplayer || (_refreshDefaultPrefabs && _instances.Count == 0)))
            {
                Generator.IgnorePostProcess = true;
                Debug.Log("DefaultPrefabCollection is being refreshed.");
                Generator.GenerateFull(initializeAdded: false);
                Generator.IgnorePostProcess = false;
            }
            #endif
            // If default prefabs then also make a new instance and sort them.
            if (isDefaultPrefabs)
            {
                DefaultPrefabObjects originalDpo = (DefaultPrefabObjects)SpawnablePrefabs;
                // If not editor then a new instance must be made and sorted.
                DefaultPrefabObjects instancedDpo = ScriptableObject.CreateInstance<DefaultPrefabObjects>();
                instancedDpo.AddObjects(originalDpo.Prefabs.ToList(), checkForDuplicates: false, initializeAdded: false);
                instancedDpo.Sort();
                SpawnablePrefabs = instancedDpo;
            }

            if (TryGetComponent<NetworkObject>(out _))
                InternalLogError($"NetworkObject component found on the NetworkManager object {gameObject.name}. This is not allowed and will cause problems. Remove the NetworkObject component from this object.");

            SpawnablePrefabs.InitializePrefabRange(0);
            SpawnablePrefabs.SetCollectionId(0);

            SetDontDestroyOnLoad();
            SetRunInBackground();
            DebugManager = GetOrCreateComponent<DebugManager>();
            TransportManager = GetOrCreateComponent<TransportManager>();

            ServerManager = GetOrCreateComponent<ServerManager>();
            ClientManager = GetOrCreateComponent<ClientManager>();
            TimeManager = GetOrCreateComponent<TimeManager>();
            SceneManager = GetOrCreateComponent<SceneManager>();
            ObserverManager = GetOrCreateComponent<ObserverManager>();
            #if THREADED_TICKSMOOTHERS
            TickSmoothingManager = GetOrCreateComponent<TickSmoothingManager>();
            #endif
            RollbackManager = GetOrCreateComponent<RollbackManager>();
            PredictionManager = GetOrCreateComponent<PredictionManager>();
            StatisticsManager = GetOrCreateComponent<StatisticsManager>();
            if (_objectPool == null)
                _objectPool = GetOrCreateComponent<DefaultObjectPool>();

            InitializeComponents();

            _instances.Add(this);

            Initialized = true;
        }

        private void Start()
        {
            ServerManager.StartForHeadless();
        }

        private void OnDestroy()
        {
            _instances.Remove(this);
        }

        /// <summary>
        /// Initializes components. To be called after all components are added.
        /// </summary>
        private void InitializeComponents()
        {
            TimeManager.InitializeOnce_Internal(this);
            TimeManager.OnLateUpdate += TimeManager_OnLateUpdate;
            TransportManager.InitializeOnce_Internal(this);

            /* There is no need to unsubscribe to either of the connection
             * state events below since components
             * will be destroyed with the NetworkManager. */

            ClientManager.InitializeOnce_Internal(this);
            ClientManager.OnClientConnectionState += ClientManager_OnClientConnectionState;

            ServerManager.InitializeOnce_Internal(this);
            ServerManager.OnServerConnectionState += ServerManager_OnServerConnectionState;

            SceneManager.InitializeOnce_Internal(this);
            ObserverManager.InitializeOnce_Internal(this);
            #if THREADED_TICKSMOOTHERS
            TickSmoothingManager.InitializeOnce_Internal(this);
            #endif
            RollbackManager.InitializeOnce_Internal(this);
            PredictionManager.InitializeOnce(this);
            StatisticsManager.InitializeOnce_Internal(this);
            _objectPool.InitializeOnce(this);
        }

        /// <summary>
        /// Called when the local server connection changes.
        /// </summary>
        private void ServerManager_OnServerConnectionState(ServerConnectionStateArgs obj) => UpdateRunInBackgroundIfApplicable();

        /// <summary>
        /// Called when the local client connection changes.
        /// </summary>
        private void ClientManager_OnClientConnectionState(ClientConnectionStateArgs obj) => UpdateRunInBackgroundIfApplicable();

        /// <summary>
        /// Updates the frame rate based on server and client status.
        /// </summary>
        internal void UpdateFramerate()
        {
            bool clientStarted = ClientManager.Started;
            bool serverStarted = ServerManager.Started;

            int frameRate = 0;
            // If both client and server are started then use whichever framerate is higher.
            if (clientStarted && serverStarted)
                frameRate = Math.Max(ServerManager.FrameRate, ClientManager.FrameRate);
            else if (clientStarted)
                frameRate = ClientManager.FrameRate;
            else if (serverStarted)
                frameRate = ServerManager.FrameRate;

            /* Make sure framerate isn't set to max on server.
             * If it is then default to tick rate. If framerate is
             * less than tickrate then also set to tickrate. */
            #if UNITY_SERVER && !UNITY_EDITOR
            ushort minimumServerFramerate = (ushort)(TimeManager.TickRate + 15);
            if (frameRate == MAXIMUM_FRAMERATE)
                frameRate = minimumServerFramerate;
            else if (frameRate < TimeManager.TickRate)
                frameRate = minimumServerFramerate;
            #endif
            // If there is a framerate to set.
            if (frameRate > 0)
                Application.targetFrameRate = frameRate;
        }

        /// <summary>
        /// Called when MonoBehaviours call LateUpdate.
        /// </summary>
        private void TimeManager_OnLateUpdate()
        {
            /* Some reason runinbackground becomes unset
             * or the setting goes ignored some times when it's set
             * in awake. Rather than try to fix or care why Unity
             * does this just set it in LateUpdate(or Update). */
            SetRunInBackground();
            // Let's object pooler do regular work.
            _objectPool.LateUpdate();
        }

        /// <summary>
        /// Returns if this NetworkManager can initialize.
        /// </summary>
        /// <param name="instanceRetained">True if this instance will be retained/kept.</param>
        /// <returns></returns>
        private bool CanPersist()
        {
            /* If allow multiple then any number of
             * NetworkManagers are allowed. Don't
             * automatically destroy any. */
            if (_persistence == PersistenceType.AllowMultiple)
                return true;

            List<NetworkManager> instances = Instances.ToList();
            // This is the first instance, it may initialize.
            if (instances.Count == 0)
                return true;

            // First instance of NM.
            NetworkManager firstInstance = instances[0];

            // If to destroy the oldest.
            if (_persistence == PersistenceType.DestroyOldest)
            {
                InternalLog($"NetworkManager on object {firstInstance.name} is being destroyed due to persistence type {_persistence}. A NetworkManager instance has been created on {gameObject.name}.");
                DestroyImmediate(firstInstance.gameObject);
                // This being the new one will persist, allow initialization.
                return true;
            }

            // If to destroy the newest.
            if (_persistence == PersistenceType.DestroyNewest)
            {
                InternalLog($"NetworkManager on object {gameObject.name} is being destroyed due to persistence type {_persistence}. A NetworkManager instance already exist on {firstInstance.name}.");
                DestroyImmediate(gameObject);
                // This one is being destroyed because its the newest.
                return false;
            }

            // Unhandled.
            InternalLog($"Persistence type of {_persistence} is unhandled on {gameObject.name}. Initialization will not proceed, and this NetworkManager instance will be destroyed.");
            DestroyImmediate(gameObject);
            return false;
        }

        /// <summary>
        /// Validates SpawnablePrefabs field and returns if validated successfully.
        /// </summary>
        /// <returns></returns>
        private bool ValidateSpawnablePrefabs(bool print)
        {
            // If null and object is in a scene.
            if (SpawnablePrefabs == null && !string.IsNullOrEmpty(gameObject.scene.name))
            {
                // First try to fetch the file, only if editor and not in play mode.
                #if UNITY_EDITOR
                if (!ApplicationState.IsPlaying())
                {
                    SpawnablePrefabs = Generator.GetDefaultPrefabObjects();
                    if (SpawnablePrefabs != null)
                    {
                        Debug.Log($"SpawnablePrefabs was set to DefaultPrefabObjects automatically on object {gameObject.name} in scene {gameObject.scene.name}.");
                        EditorUtility.SetDirty(this);
                        return true;
                    }
                }
                #endif
                // Always throw an error as this would cause failure.
                if (print)
                    Debug.LogError($"SpawnablePrefabs is null on {gameObject.name}. Select the NetworkManager in scene {gameObject.scene.name} and choose a prefabs file. Choosing DefaultPrefabObjects will automatically populate prefabs for you.");
                return false;
            }

            return true;
        }

        /// <summary>
        /// Sets DontDestroyOnLoad if configured to.
        /// </summary>
        private void SetDontDestroyOnLoad()
        {
            if (_dontDestroyOnLoad)
                DontDestroyOnLoad(this);
        }

        /// <summary>
        /// Sets Application.runInBackground to runInBackground.
        /// </summary>
        private void SetRunInBackground()
        {
            Application.runInBackground = _runInBackground;
        }

        /// <summary>
        /// Gets a component, creating and adding it if it does not exist.
        /// </summary>
        /// <param name = "presetValue">Value which may already be set. When not null this is returned instead.</param>
        private T GetOrCreateComponent<T>(T presetValue = null) where T : UnityEngine.Component
        {
            // If already set then return set value.
            if (presetValue != null)
                return presetValue;

            if (gameObject.TryGetComponent(out T result))
                return result;
            else
                return gameObject.AddComponent<T>();
        }

        /// <summary>
        /// Clears a client collection after disposing of the NetworkConnections.
        /// </summary>
        /// <param name = "clients"></param>
        internal void ClearClientsCollection(Dictionary<int, NetworkConnection> clients, int transportIndex = -1)
        {
            // True to dispose all connections.
            bool disposeAll = transportIndex < 0;
            List<int> cache = CollectionCaches<int>.RetrieveList();

            /* Only reset NetworkConnections if server is also not started.
             * Otherwise, this would reset connections for the server side
             * as well. */
            bool canResetState = !IsServerStarted;

            foreach (KeyValuePair<int, NetworkConnection> kvp in clients)
            {
                NetworkConnection value = kvp.Value;
                // If to check transport index.
                if (!disposeAll)
                {
                    if (value.TransportIndex == transportIndex)
                    {
                        cache.Add(kvp.Key);
                        if (canResetState)
                            value.ResetState();
                    }
                }
                // Not using transport index, no check required.
                else
                {
                    if (canResetState)
                        value.ResetState();
                }
            }

            // If all are being disposed the collection can be cleared.
            if (disposeAll)
            {
                clients.Clear();
            }
            // Otherwise, only remove those which were disposed.
            else
            {
                foreach (int item in cache)
                    clients.Remove(item);
            }

            CollectionCaches<int>.Store(cache);
        }

        /// <summary>
        /// Updates Application.RunInBackground to NetworkManager setting when connected, and application setting when not connected.
        /// </summary>
        private void UpdateRunInBackgroundIfApplicable()
        {
            // Not configured to update values.
            if (!_runInBackground)
                return;

            bool anythingStarted = ServerManager.IsAnyServerStarted() || ClientManager.Started;

            // Check to set values.
            if (anythingStarted)
            {
                // Already set.
                if (_offlineApplicationRunInBackground != null)
                    return;

                //Update run in background after caching current value.
                _offlineApplicationRunInBackground = Application.runInBackground;
                Application.runInBackground = true;
            }
            else
            {
                // Already unset.
                if (_offlineApplicationRunInBackground == null)
                    return;

                //Update run in background then unset cached value.
                Application.runInBackground = _offlineApplicationRunInBackground.Value;
                _offlineApplicationRunInBackground = null;
            }
        }

        #region Editor.
        #if UNITY_EDITOR
        private void OnValidate()
        {
            if (SpawnablePrefabs == null)
                Reset();
        }

        private void Reset()
        {
            ValidateSpawnablePrefabs(true);
        }

        #endif
        #endregion
    }
}
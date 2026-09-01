#if UNITY_EDITOR || DEVELOPMENT_BUILD
#define DEVELOPMENT
#endif
using FishNet.CodeGenerating;
using FishNet.Documenting;
using FishNet.Managing.Transporting;
using FishNet.Serializing.Helping;
using FishNet.Utility;
using System.Runtime.CompilerServices;
using FishNet.Managing;
using FishNet.Managing.Statistic;
using UnityEngine;

[assembly: InternalsVisibleTo(UtilityConstants.CODEGEN_ASSEMBLY_NAME)]

namespace FishNet.Object
{
    /// <summary>
    /// Scripts which inherit from NetworkBehaviour can be used to gain insight of, and perform actions on the network.
    /// </summary>
    [ExcludeSerialization]
    public abstract partial class NetworkBehaviour : MonoBehaviour
    {
        #region Public.
        /// <summary>
        /// True if this NetworkBehaviour is initialized for the network.
        /// </summary>
        public bool IsSpawned => _networkObjectCache.IsSpawned;
        /// <summary>
        /// </summary>
        [SerializeField]
        [HideInInspector]
        private byte _componentIndexCache = UNSET_NETWORKBEHAVIOUR_ID;
        /// <summary>
        /// ComponentIndex for this NetworkBehaviour.
        /// </summary>
        public byte ComponentIndex
        {
            get => _componentIndexCache;
            private set => _componentIndexCache = value;
        }
        #if UNITY_EDITOR
        /// <summary>
        /// NetworkObject automatically added or discovered during edit time.
        /// </summary>
        [SerializeField]
        [HideInInspector]
        private NetworkObject _addedNetworkObject;
        #endif
        /// <summary>
        /// Cache of the TransportManager.
        /// </summary>
        private TransportManager _transportManagerCache;
        /// <summary>
        /// </summary>
        [SerializeField]
        [HideInInspector]
        private NetworkObject _networkObjectCache;
        /// <summary>
        /// NetworkObject this behaviour is for.
        /// </summary>
        public NetworkObject NetworkObject => _networkObjectCache;
        #endregion

        #region Private.
        /// <summary>
        /// True if initialized at some point asServer.
        /// </summary>
        #pragma warning disable CS0414 // Field is assigned but its value is never used
        private bool _initializedOnceServer;
        /// <summary>
        /// True if initialized at some point not asServer.
        /// </summary>
        private bool _initializedOnceClient;
        #pragma warning restore CS0414 // Field is assigned but its value is never used
        #if !UNITY_SERVER
        /// <summary>
        /// </summary>
        private NetworkTrafficStatistics _networkTrafficStatistics;
        /// <summary>
        /// Name of this NetworkBehaviour.
        /// </summary>
        private string _typeName = string.Empty;
        #endif
        #endregion

        #region Consts.
        /// <summary>
        /// Maximum number of allowed added NetworkBehaviours.
        /// </summary>
        public const byte MAXIMUM_NETWORKBEHAVIOURS = UNSET_NETWORKBEHAVIOUR_ID - 1;
        /// <summary>
        /// Id for when a NetworkBehaviour is not valid.
        /// </summary>
        public const byte UNSET_NETWORKBEHAVIOUR_ID = byte.MaxValue;
        #endregion

        /// <summary>
        /// Outputs data about this NetworkBehaviour to string.
        /// </summary>
        /// <returns></returns>
        public override string ToString()
        {
            return $"Name [{gameObject.name}] ComponentId [{ComponentIndex}] NetworkObject Name [{_networkObjectCache.name}] NetworkObject Id [{_networkObjectCache.ObjectId}]";
        }

        [MakePublic]
        internal virtual void NetworkInitialize___Early() { }

        [MakePublic]
        internal virtual void NetworkInitialize___Late() { }

        /// <summary>
        /// Preinitializes this script for the network.
        /// </summary>
        internal void InitializeEarly(NetworkObject nob, bool asServer)
        {
            #if DEVELOPMENT && !UNITY_SERVER
            if (_typeName == string.Empty)
                _typeName = GetType().Name;
            #endif

            _transportManagerCache = nob.TransportManager;
            SyncTypes_Preinitialize(asServer);

            #if DEVELOPMENT && !UNITY_SERVER
            nob.NetworkManager.StatisticsManager.TryGetNetworkTrafficStatistics(out _networkTrafficStatistics);
            #endif
            
            if (asServer)
            {
                InitializeRpcLinks();
                _initializedOnceServer = true;
            }
            else
            {
                _initializedOnceClient = true;
            }
        }

        internal void Deinitialize(bool asServer)
        {
            ResetState_SyncTypes(asServer);
        }

        /// <summary>
        /// Called by the NetworkObject when this object is destroyed.
        /// </summary>
        internal void NetworkBehaviour_OnDestroy()
        {
            SyncTypes_OnDestroy();
        }

        /// <summary>
        /// Serializes information for network components.
        /// </summary>
        internal void SerializeComponents(NetworkObject nob, byte componentIndex)
        {
            _networkObjectCache = nob;
            ComponentIndex = componentIndex;
        }

        /// <summary>
        /// Manually initializes network content for the NetworkBehaviour if the object it's on is disabled.
        /// </summary>
        internal void InitializeIfDisabled()
        {
            if (gameObject.activeInHierarchy)
                return;

            NetworkInitializeIfDisabled();
        }

        /// <summary>
        /// Long name is to prevent users from potentially creating their own method named the same.
        /// </summary>
        [MakePublic]
        [APIExclude]
        internal virtual void NetworkInitializeIfDisabled() { }

        #region Editor.
        protected virtual void Reset()
        {
            #if UNITY_EDITOR
            if (Application.isPlaying)
                return;

            TryAddNetworkObject();
            #endif
        }

        protected virtual void OnValidate()
        {
            #if UNITY_EDITOR
            if (Application.isPlaying)
                return;

            TryAddNetworkObject();
            #endif
        }

        /// <summary>
        /// Resets this NetworkBehaviour so that it may be added to an object pool.
        /// </summary>
        public virtual void ResetState(bool asServer)
        {
            ResetState_SyncTypes(asServer);
            ResetState_Prediction(asServer);
            ClearReplicateCache();
            ClearBuffedRpcs();
        }

        /// <summary>
        /// Tries to add the NetworkObject component.
        /// </summary>
        private NetworkObject TryAddNetworkObject()
        {
            #if UNITY_EDITOR
            if (Application.isPlaying)
                return _addedNetworkObject;

            if (_addedNetworkObject != null)
            {
                AlertToDuplicateNetworkObjects(_addedNetworkObject.transform);
                return _addedNetworkObject;
            }

            /* Manually iterate up the chain because GetComponentInParent doesn't
             * work when modifying prefabs in the inspector. Unity, you're starting
             * to suck a lot right now. */
            NetworkObject result = null;
            Transform climb = transform;

            while (climb != null)
            {
                if (climb.TryGetComponent(out result))
                    break;
                else
                    climb = climb.parent;
            }

            if (result != null)
            {
                _addedNetworkObject = result;
            }
            // Not found, add a new nob.
            else
            {
                _addedNetworkObject = transform.root.gameObject.AddComponent<NetworkObject>();
                NetworkManagerExtensions.Log($"Script {GetType().Name} on object {gameObject.name} added a NetworkObject component to {transform.root.name}.");
            }

            AlertToDuplicateNetworkObjects(_addedNetworkObject.transform);
            return _addedNetworkObject;

            // Removes duplicate network objects from t.
            void AlertToDuplicateNetworkObjects(Transform t)
            {
                NetworkObject[] nobs = t.GetComponents<NetworkObject>();
                // This shouldn't be possible but does occur sometimes; maybe a unity bug?
                if (nobs.Length > 1)
                {
                    // Update added to first entryt.
                    _addedNetworkObject = nobs[0];

                    string useMenu = " You may also use the Fish-Networking menu to automatically remove duplicate NetworkObjects.";
                    string sceneName = t.gameObject.scene.name;
                    if (string.IsNullOrEmpty(sceneName))
                        Debug.LogError($"Prefab {t.name} has multiple NetworkObject components. Please remove the extra component(s) to prevent errors.{useMenu}");
                    else
                        Debug.LogError($"Object {t.name} in scene {sceneName} has multiple NetworkObject components. Please remove the extra component(s) to prevent errors.{useMenu}");
                }
            }
            #else
            return null;
            #endif
        }
        #endregion
    }
}
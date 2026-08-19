using FishNet.Component.ColliderRollback;
using FishNet.Managing;
using FishNet.Managing.Client;
using FishNet.Managing.Predicting;
using FishNet.Managing.Scened;
using FishNet.Managing.Server;
using FishNet.Managing.Statistic;
using FishNet.Managing.Timing;
using FishNet.Managing.Transporting;
using FishNet.Utility;
using GameKit.Dependencies.Utilities;
using System;
using System.Linq;
using UnityEngine;

namespace FishNet
{
    /// <summary>
    /// Used to globally get information from the first found instance of NetworkManager.
    /// </summary>
    public static class InstanceFinder
    {
        #region Public.
        /// <summary>
        /// Returns the first found NetworkManager instance.
        /// </summary>
        public static NetworkManager NetworkManager
        {
            get
            {
                if (_networkManager == null)
                {
                    int managersCount = NetworkManager.Instances.Count;
                    // At least one manager.
                    if (managersCount > 0)
                    {
                        _networkManager = NetworkManager.Instances[0];
                        if (managersCount > 1)
                            _networkManager.LogWarning($"Multiple NetworkManagers found, the first result will be returned. If you only wish to have one NetworkManager then uncheck 'Allow Multiple' within your NetworkManagers.");
                    }
                    // No managers.
                    else
                    {
                        // If application is quitting return null without logging.
                        if (ApplicationState.IsQuitting())
                            return null;

                        // Do not log using NetworkManager extensions, it will try to use InstanceFinder, resulting in this causing a stack overflow.
                        Debug.Log($"NetworkManager not found in any open scenes.");
                    }
                }

                return _networkManager;
            }
        }
        /// <summary>
        /// Returns the first instance of ServerManager.
        /// </summary>
        public static ServerManager ServerManager
        {
            get
            {
                NetworkManager nm = NetworkManager;
                return nm == null ? null : nm.ServerManager;
            }
        }
        /// <summary>
        /// Returns the first instance of ClientManager.
        /// </summary>
        public static ClientManager ClientManager
        {
            get
            {
                NetworkManager nm = NetworkManager;
                return nm == null ? null : nm.ClientManager;
            }
        }
        /// <summary>
        /// Returns the first instance of TransportManager.
        /// </summary>
        public static TransportManager TransportManager
        {
            get
            {
                NetworkManager nm = NetworkManager;
                return nm == null ? null : nm.TransportManager;
            }
        }
        /// <summary>
        /// Returns the first instance of TimeManager.
        /// </summary>
        public static TimeManager TimeManager
        {
            get
            {
                NetworkManager nm = NetworkManager;
                return nm == null ? null : nm.TimeManager;
            }
        }
        /// <summary>
        /// Returns the first instance of SceneManager.
        /// </summary>
        public static SceneManager SceneManager
        {
            get
            {
                NetworkManager nm = NetworkManager;
                return nm == null ? null : nm.SceneManager;
            }
        }
        /// <summary>
        /// Returns the first instance of RollbackManager.
        /// </summary>
        public static RollbackManager RollbackManager
        {
            get
            {
                NetworkManager nm = NetworkManager;
                return nm == null ? null : nm.RollbackManager;
            }
        }
        /// <summary>
        /// Returns the first instance of PredictionManager.
        /// </summary>
        public static PredictionManager PredictionManager
        {
            get
            {
                NetworkManager nm = NetworkManager;
                return nm == null ? null : nm.PredictionManager;
            }
        }
        /// <summary>
        /// Returns the first instance of StatisticsManager.
        /// </summary>
        public static StatisticsManager StatisticsManager
        {
            get
            {
                NetworkManager nm = NetworkManager;
                return nm == null ? null : nm.StatisticsManager;
            }
        }

        #region Obsoletes
        [Obsolete("Use IsClientOnlyStarted. Note the difference between IsClientOnlyInitialized and IsClientOnlyStarted.")]
        public static bool IsClientOnly => IsClientOnlyStarted;
        [Obsolete("Use IsServerOnlyStarted. Note the difference between IsServerOnlyInitialized and IsServerOnlyStarted.")]
        public static bool IsServerOnly => IsServerOnlyStarted;
        [Obsolete("Use IsHostStarted. Note the difference between IsHostInitialized and IsHostStarted.")]
        public static bool IsHost => IsHostStarted;
        [Obsolete("Use IsClientStarted. Note the difference between IsClientInitialized and IsClientStarted.")]
        public static bool IsClient => IsClientStarted;
        [Obsolete("Use IsServerStarted. Note the difference between IsServerInitialized and IsServerStarted.")]
        public static bool IsServer => IsServerStarted;
        #endregion

        /// <summary>
        /// True if the server is active.
        /// </summary>
        public static bool IsServerStarted => NetworkManager == null ? false : NetworkManager.IsServerStarted;
        /// <summary>
        /// True if only the server is started.
        /// </summary>
        public static bool IsServerOnlyStarted => NetworkManager == null ? false : NetworkManager.IsServerOnlyStarted;
        /// <summary>
        /// True if the client is started and authenticated.
        /// </summary>
        public static bool IsClientStarted => NetworkManager == null ? false : NetworkManager.IsClientStarted;
        /// <summary>
        /// True if only the client is started and authenticated.
        /// </summary>
        public static bool IsClientOnlyStarted => NetworkManager == null ? false : NetworkManager.IsClientOnlyStarted;
        /// <summary>
        /// True if client and server are started.
        /// </summary>
        public static bool IsHostStarted => NetworkManager == null ? false : NetworkManager.IsHostStarted;
        /// <summary>
        /// True if client nor server are started.
        /// </summary>
        public static bool IsOffline
        {
            get { return NetworkManager == null ? true : NetworkManager.IsOffline; }
        }
        #endregion

        #region Private.
        /// <summary>
        /// NetworkManager instance.
        /// </summary>
        private static NetworkManager _networkManager;
        #endregion

        #region Registered components
        /// <summary>
        /// Registers to invoke an action when a specified component becomes registered. Action will invoke immediately if already registered.
        /// </summary>
        /// <typeparam name = "T">Component type.</typeparam>
        /// <param name = "handler">Action to invoke.</param>
        public static void RegisterInvokeOnInstance<T>(Action<UnityEngine.Component> handler) where T : UnityEngine.Component
        {
            if (NetworkManager != null)
                NetworkManager.RegisterInvokeOnInstance<T>(handler);
        }

        /// <summary>
        /// Unrgisters to invoke an action when a specified component becomes registered. Action will invoke immediately if already registered.
        /// </summary>
        /// <typeparam name = "T">Component type.</typeparam>
        /// <param name = "handler">Action to invoke.</param>
        public static void UnregisterInvokeOnInstance<T>(Action<UnityEngine.Component> handler) where T : UnityEngine.Component
        {
            if (NetworkManager != null)
                NetworkManager.UnregisterInvokeOnInstance<T>(handler);
        }

        /// <summary>
        /// Returns class of type if found within CodegenBase classes.
        /// </summary>
        /// <typeparam name = "T"></typeparam>
        /// <returns></returns>
        public static T GetInstance<T>() where T : UnityEngine.Component
        {
            return NetworkManager == null ? default : NetworkManager.GetInstance<T>();
        }

        /// <summary>
        /// Returns if class of type is registered with the NetworkManager.
        /// </summary>
        /// <typeparam name = "T">Type to check for.</typeparam>
        /// <returns></returns>
        public static bool HasInstance<T>() where T : UnityEngine.Component
        {
            return NetworkManager == null ? false : NetworkManager.HasInstance<T>();
        }

        /// <summary>
        /// Registers a new component to this NetworkManager.
        /// </summary>
        /// <typeparam name = "T">Type to register.</typeparam>
        /// <param name = "component">Reference of the component being registered.</param>
        /// <param name = "replace">True to replace existing references.</param>
        public static void RegisterInstance<T>(T component, bool replace = true) where T : UnityEngine.Component
        {
            if (NetworkManager != null)
                NetworkManager.RegisterInstance(component, replace);
        }

        /// <summary>
        /// Tries to registers a new component to this NetworkManager.
        /// This will not register the instance if another already exists.
        /// </summary>
        /// <typeparam name = "T">Type to register.</typeparam>
        /// <param name = "component">Reference of the component being registered.</param>
        /// <returns>True if was able to register, false if an instance is already registered.</returns>
        public static bool TryRegisterInstance<T>(T component) where T : UnityEngine.Component
        {
            return NetworkManager == null ? false : NetworkManager.TryRegisterInstance(component);
        }

        /// <summary>
        /// Returns class of type from registered instances.
        /// </summary>
        /// <param name = "component">Outputted component.</param>
        /// <typeparam name = "T">Type to get.</typeparam>
        /// <returns>True if was able to get instance.</returns>
        public static bool TryGetInstance<T>(out T component) where T : UnityEngine.Component
        {
            if (NetworkManager == null)
            {
                component = default;
                return false;
            }
            else
            {
                return NetworkManager.TryGetInstance(out component);
            }
        }

        /// <summary>
        /// Unregisters a component from this NetworkManager.
        /// </summary>
        /// <typeparam name = "T">Type to unregister.</typeparam>
        public static void UnregisterInstance<T>() where T : UnityEngine.Component
        {
            if (NetworkManager != null)
                NetworkManager.UnregisterInstance<T>();
        }
        #endregion
    }
}
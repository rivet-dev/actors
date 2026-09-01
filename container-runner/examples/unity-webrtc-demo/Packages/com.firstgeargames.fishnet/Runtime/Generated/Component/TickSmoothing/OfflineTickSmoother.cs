using FishNet.Managing;
using FishNet.Managing.Timing;
using GameKit.Dependencies.Utilities;
using UnityEngine;
using UnityEngine.Serialization;

namespace FishNet.Component.Transforming.Beta
{
    /// <summary>
    /// Smoothes this object between ticks.
    /// </summary>
    /// <remarks>This can be configured to smooth over a set interval of time, or to smooth adaptively and make path corrections for prediction.</remarks>
    public class OfflineTickSmoother : MonoBehaviour
    {
        #region Public.
        /// <summary>
        /// Logic for owner smoothing.
        /// </summary>
        public TickSmootherController SmootherController { get; private set; }
        /// <summary>
        /// True if this component is initialized.
        /// </summary>
        /// <remarks>This API is for internal use and may change at any time.</remarks>
        public bool IsInitialized { get; private set; }
        #endregion

        #region Serialized.
        /// <summary>
        /// True to automatically initialize in Awake using InstanceFinder. When false you will need to manually call Initialize.
        /// </summary>
        [Tooltip("True to automatically initialize in Awake using InstanceFinder. When false you will need to manually call Initialize.")]
        [SerializeField]
        private bool _automaticallyInitialize = true;
        /// <summary>
        /// Settings required to initialize the smoother.
        /// </summary>
        [Tooltip("Settings required to initialize the smoother.")]
        [SerializeField]
        private InitializationSettings _initializationSettings = new();
        /// <summary>
        /// How smoothing occurs when the controller of the object.
        /// </summary>
        [FormerlySerializedAs("_controllerMovementSettings")]
        [Tooltip("How smoothing occurs when the controller of the object.")]
        [SerializeField]
        private MovementSettings _movementSettings = new(true);
        #endregion

        private void Awake()
        {
            RetrieveControllers();
            AutomaticallyInitialize();
        }

        private void OnDestroy()
        {
            if (SmootherController != null)
            {
                SmootherController.StopSmoother();
                SmootherController.OnDestroy();
            }

            StoreControllers();

            IsInitialized = false;
        }

        /// <summary>
        /// Automatically initializes if feature is enabled.
        /// </summary>
        private void AutomaticallyInitialize()
        {
            if (!_automaticallyInitialize)
                return;

            TimeManager tm = InstanceFinder.TimeManager;
            if (tm == null)
            {
                NetworkManagerExtensions.LogWarning($"Automatic initialization failed on {gameObject.name}. You must manually call Initialize.");
                return;
            }

            Initialize(tm);
        }

        /// <summary>
        /// Initializes using a specified TimeManager.
        /// </summary>
        /// <param name = "timeManager"></param>
        public void Initialize(TimeManager timeManager)
        {
            if (timeManager == null)
            {
                NetworkManagerExtensions.LogError($"TimeManager cannot be null when initializing.");
                return;
            }

            SmootherController.SetTimeManager(timeManager);

            _initializationSettings.SetOfflineRuntimeValues(timeManager, graphicalTransform: transform);
            SmootherController.Initialize(_initializationSettings, _movementSettings, default);
            SmootherController.StartSmoother();

            IsInitialized = true;
        }

        /// <summary>
        /// Sets a transform as the target to follow.
        /// </summary>
        /// <param name = "value">New value.</param>
        public void SetTargetTransform(Transform value)
        {
            if (IsInitialized)
            {
                NetworkManagerExtensions.LogError($"Target can only be set before Initialize is called.");
                return;
            }

            _initializationSettings.TargetTransform = value;
        }

        /// <summary>
        /// Stores smoothers if they have value.
        /// </summary>
        private void StoreControllers()
        {
            if (SmootherController == null)
                return;

            ResettableObjectCaches<TickSmootherController>.Store(SmootherController);
            SmootherController = null;
        }

        /// <summary>
        /// Stores current smoothers and retrieves new ones.
        /// </summary>
        private void RetrieveControllers()
        {
            StoreControllers();
            SmootherController = ResettableObjectCaches<TickSmootherController>.Retrieve();
        }
    }
}
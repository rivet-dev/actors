using FishNet.Managing.Logging;
using FishNet.Managing.Timing;
using FishNet.Object;
using FishNet.Object.Prediction;
using GameKit.Dependencies.Utilities;
using Unity.Profiling;
using UnityEngine;

#pragma warning disable CS0618 // Type or member is obsolete

namespace FishNet.Component.Transforming
{
    /// <summary>
    /// Smoothes an object between ticks.
    /// This can be used on objects without NetworkObject components.
    /// </summary>
    public class MonoTickSmoother : MonoBehaviour
    {
        // Lazy way to display obsolete message w/o using a custom editor.
        [Header("This component will be obsoleted soon.")]
        [Header("Use NetworkTickSmoother or OfflineTickSmoother.")]
        [Header(" ")]

        #region Serialized.
        /// <summary>
        /// True to use InstanceFinder to locate the TimeManager. When false specify which TimeManager to use by calling SetTimeManager.
        /// </summary>
        [Tooltip("True to use InstanceFinder to locate the TimeManager. When false specify which TimeManager to use by calling SetTimeManager.")]
        [SerializeField]
        private bool _useInstanceFinder = true;
        /// <summary>
        /// GraphicalObject you wish to smooth.
        /// </summary>
        [Tooltip("GraphicalObject you wish to smooth.")]
        [SerializeField]
        private Transform _graphicalObject;
        /// <summary>
        /// True to enable teleport threshhold.
        /// </summary>
        [Tooltip("True to enable teleport threshold.")]
        [SerializeField]
        private bool _enableTeleport;
        /// <summary>
        /// How far the object must move between ticks to teleport rather than smooth.
        /// </summary>
        [Tooltip("How far the object must move between ticks to teleport rather than smooth.")]
        [Range(0f, ushort.MaxValue)]
        [SerializeField]
        private float _teleportThreshold;
        #endregion

        #region Private.
        /// <summary>
        /// TimeManager subscribed to.
        /// </summary>
        private TimeManager _timeManager;
        /// <summary>
        /// BasicTickSmoother for this script.
        /// </summary>
        private LocalTransformTickSmoother _tickSmoother;
        #endregion
        
        #region Private Profiler Markers
        
        private static readonly ProfilerMarker _pm_OnPreTick = new("MonoTickSmoother._timeManager_OnPreTick()");
        private static readonly ProfilerMarker _pm_OnPostTick = new("MonoTickSmoother._timeManager_OnPostTick()");
        
        #endregion

        private void OnEnable()
        {
            Initialize();
        }

        private void OnDisable()
        {
            _tickSmoother.ResetState();
            ChangeSubscription(false);
            ObjectCaches<LocalTransformTickSmoother>.StoreAndDefault(ref _tickSmoother);
        }

        [Client(Logging = LoggingType.Off)]
        private void Update()
        {
            _tickSmoother?.Update();
        }

        /// <summary>
        /// Initializes this script for use.
        /// </summary>
        private void Initialize()
        {
            _tickSmoother = ObjectCaches<LocalTransformTickSmoother>.Retrieve();
            if (_useInstanceFinder)
            {
                _timeManager = InstanceFinder.TimeManager;
                ChangeSubscription(true);
            }
        }

        /// <summary>
        /// Sets a new PredictionManager to use.
        /// </summary>
        /// <param name = "tm"></param>
        public void SetTimeManager(TimeManager tm)
        {
            if (tm == _timeManager)
                return;

            // Unsub from current.
            ChangeSubscription(false);
            // Sub to newest.
            _timeManager = tm;
            ChangeSubscription(true);
        }

        /// <summary>
        /// Changes the subscription to the TimeManager.
        /// </summary>
        private void ChangeSubscription(bool subscribe)
        {
            if (_timeManager == null)
                return;

            if (subscribe)
            {
                if (_tickSmoother != null)
                {
                    float tDistance = _enableTeleport ? _teleportThreshold : MoveRates.UNSET_VALUE;
                    _tickSmoother.InitializeOnce(_graphicalObject, tDistance, (float)_timeManager.TickDelta, 1);
                }
                _timeManager.OnPreTick += _timeManager_OnPreTick;
                _timeManager.OnPostTick += _timeManager_OnPostTick;
            }
            else
            {
                _timeManager.OnPreTick -= _timeManager_OnPreTick;
                _timeManager.OnPostTick -= _timeManager_OnPostTick;
            }
        }

        /// <summary>
        /// Called before a tick starts.
        /// </summary>
        private void _timeManager_OnPreTick()
        {
            using (_pm_OnPreTick.Auto())
            {
                _tickSmoother.OnPreTick();
            }
        }

        /// <summary>
        /// Called after a tick completes.
        /// </summary>
        private void _timeManager_OnPostTick()
        {
            using (_pm_OnPostTick.Auto())
            {
                _tickSmoother.OnPostTick();
            }
        }
    }
}
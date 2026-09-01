using FishNet.Object;
using GameKit.Dependencies.Utilities;
using UnityEngine;

namespace FishNet.Component.Transforming.Beta
{
    /// <summary>
    /// Smoothes this object between ticks.
    /// </summary>
    /// <remarks>This can be configured to smooth over a set interval of time, or to smooth adaptively and make path corrections for prediction.</remarks>
    public class NetworkTickSmoother : NetworkBehaviour
    {
        #region Public.
        /// <summary>
        /// Logic for owner smoothing.
        /// </summary>
        public TickSmootherController SmootherController { get; private set; }
        #endregion

        /// <summary>
        /// Settings required to initialize the smoother.
        /// </summary>
        [Tooltip("Settings required to initialize the smoother.")]
        [SerializeField]
        private InitializationSettings _initializationSettings = new();
        /// <summary>
        /// How smoothing occurs when the controller of the object.
        /// </summary>
        [Tooltip("How smoothing occurs when the controller of the object.")]
        [SerializeField]
        private MovementSettings _controllerMovementSettings = new(true);
        /// <summary>
        /// True to disable smoothing when the NetworkObject enables prediction, specifies a NetworkTransform to use, and that NetworkTransform is currently smoothing.
        /// </summary>
        [Tooltip("True to disable smoothing when the NetworkObject enables prediction, specifies a NetworkTransform to use, and that NetworkTransform is currently smoothing.")]
        [SerializeField]
        private bool _favorPredictionNetworkTransform = true;
        /// <summary>
        /// How smoothing occurs when spectating the object.
        /// </summary>
        [Tooltip("How smoothing occurs when spectating the object.")]
        [SerializeField]
        private MovementSettings _spectatorMovementSettings = new(true);

        private void OnDestroy()
        {
            if (SmootherController != null)
                SmootherController.OnDestroy();
            StoreControllers();
        }

        public override void OnStartClient()
        {
            RetrieveControllers();

            _initializationSettings.SetNetworkedRuntimeValues(initializingNetworkBehaviour: this, graphicalTransform: transform, _favorPredictionNetworkTransform);
            SmootherController.Initialize(_initializationSettings, _controllerMovementSettings, _spectatorMovementSettings);

            SmootherController.StartSmoother();
        }

        public override void OnStopClient()
        {
            if (SmootherController == null)
                return;

            SmootherController.StopSmoother();
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
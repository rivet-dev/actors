using FishNet.Managing.Predicting;
using UnityEngine;

namespace FishNet.Component.Prediction
{
    public partial class OfflineRigidbody : MonoBehaviour
    {
        #region Serialized.
        /// <summary>
        /// Type of prediction movement which is being used.
        /// </summary>
        [Tooltip("Type of prediction movement which is being used.")]
        [SerializeField]
        private RigidbodyType _rigidbodyType;
        /// <summary>
        /// True to also get rigidbody components within children.
        /// </summary>
        [Tooltip("True to also get rigidbody components within children.")]
        [SerializeField]
        private bool _getInChildren;
        #endregion

        #region Private.
        /// <summary>
        /// Pauser for rigidbodies.
        /// </summary>
        private RigidbodyPauser _rigidbodyPauser = new();
        /// <summary>
        /// TimeManager subscribed to.
        /// </summary>
        private PredictionManager _predictionManager;
        #endregion

        private void Awake()
        {
            InitializeOnce();
        }

        private void OnDestroy()
        {
            ChangeSubscription(false);
        }

        /// <summary>
        /// Initializes this script for use.
        /// </summary>
        private void InitializeOnce()
        {
            _predictionManager = InstanceFinder.PredictionManager;
            UpdateRigidbodies();
            ChangeSubscription(true);
        }

        /// <summary>
        /// Sets a new PredictionManager to use.
        /// </summary>
        /// <param name = "tm"></param>
        public void SetPredictionManager(PredictionManager pm)
        {
            if (pm == _predictionManager)
                return;

            // Unsub from current.
            ChangeSubscription(false);
            // Sub to newest.
            _predictionManager = pm;
            ChangeSubscription(true);
        }

        /// <summary>
        /// Finds and assigns rigidbodie using configured settings.
        /// </summary>
        public void UpdateRigidbodies()
        {
            _rigidbodyPauser.UpdateRigidbodies(transform, _rigidbodyType, _getInChildren);
        }

        /// <summary>
        /// Changes the subscription to the TimeManager.
        /// </summary>
        private void ChangeSubscription(bool subscribe)
        {
            if (_predictionManager == null)
                return;

            if (subscribe)
            {
                _predictionManager.OnPreReconcile += _predictionManager_OnPreReconcile;
                _predictionManager.OnPostReconcile += _predictionManager_OnPostReconcile;
            }
            else
            {
                _predictionManager.OnPreReconcile -= _predictionManager_OnPreReconcile;
                _predictionManager.OnPostReconcile -= _predictionManager_OnPostReconcile;
            }
        }

        private void _predictionManager_OnPreReconcile(uint clientTick, uint serverTick)
        {
            _rigidbodyPauser.Pause();
        }

        private void _predictionManager_OnPostReconcile(uint clientTick, uint serverTick)
        {
            _rigidbodyPauser.Unpause();
        }
    }
}
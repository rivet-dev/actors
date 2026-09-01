using GameKit.Dependencies;
using System.Runtime.CompilerServices;
using Sirenix.OdinInspector;
using UnityEngine;

namespace GameKit.Dependencies.Utilities.Types.CanvasContainers
{
    public class FloatingContainer : CanvasGroupFader
    {
        #region Serialized.
        /// <summary>
        /// RectTransform to move.
        /// </summary>
        [Tooltip("RectTransform to move.")]
        [SerializeField]
        [TabGroup("Components")]
        protected RectTransform RectTransform;
        /// <summary>
        /// True to use edge avoidance.
        /// </summary>
        [Tooltip("True to use edge avoidance.")]
        [SerializeField]
        [TabGroup("Sizing")]
        protected bool UseEdgeAvoidance = true;
        /// <summary>
        /// How much to avoid screen edges when being moved.
        /// </summary>
        [Tooltip("How much to avoid screen edges when being moved.")]
        [SerializeField]
        [TabGroup("Sizing")]
        [ShowIf(nameof(UseEdgeAvoidance), true)]
        protected Vector2 EdgeAvoidance;
        #endregion

        #region Private.
        /// <summary>
        /// Desired position.
        /// </summary>
        private Vector3 _positionGoal;
        /// <summary>
        /// Desired rotation.
        /// </summary>
        private Quaternion _rotationGoal;
        /// <summary>
        /// Desired scale.
        /// </summary>
        private Vector3 _scaleGoal = Vector3.one;
        /// <summary>
        /// How much edge avoidance to use.
        /// </summary>
        private Vector2? _edgeAvoidance;
        #endregion

        /// <summary>
        /// Attachs a gameObject as a child of this object and sets transform valus to default.
        /// </summary>
        /// <param name = "go">GameObject to attach.</param>
        public void AttachGameObject(GameObject go)
        {
            if (go == null)
                return;

            Transform goT = go.transform;
            goT.SetParent(transform);
            goT.localPosition = Vector3.zero;
            goT.localRotation = Quaternion.identity;
            goT.localScale = Vector3.one;
        }

        /// <summary>
        /// Shows the container.
        /// </summary>
        /// <param name = "position">Position to use.</param>
        /// <param name = "rotation">Rotation to use.</param>
        /// <param name = "scale">Scale to use.</param>
        /// <param name = "pivot">Pivot for rectTransform.</param>
        /// <param name = "edgeAvoidanceOverride">How far to keep the RectTransform from the edge. If null serialized avoidance will be used.</param>
        public virtual void Show(Vector3 position, Quaternion rotation, Vector3 scale, Vector2 pivot, Vector2? edgeAvoidanceOverride = null)
        {
            UpdateEdgeAvoidance(edgeAvoidanceOverride, false);
            UpdatePivot(pivot, false);
            UpdatePositionRotationAndScale(position, rotation, scale);
            base.Show();
        }

        /// <summary>
        /// Shows the container.
        /// </summary>
        /// <param name = "position">Position to use.</param>
        /// <param name = "edgeAvoidanceOverride">How far to keep the RectTransform from the edge. If null serialized avoidance will be used.</param>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public virtual void Show(Vector3 position, Vector2? edgeAvoidanceOverride = null)
        {
            Show(position, Quaternion.identity, Vector3.one, RectTransform.pivot);
        }

        /// <summary>
        /// Shows the container.
        /// </summary>
        /// <param name = "position">Position to use.</param>
        /// <param name = "rotation">Rotation to use.</param>
        /// <param name = "edgeAvoidanceOverride">How far to keep the RectTransform from the edge. If null serialized avoidance will be used.</param>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public virtual void Show(Vector3 position, Quaternion rotation, Vector2? edgeAvoidanceOverride = null)
        {
            Show(position, rotation, Vector3.one, RectTransform.pivot);
        }

        /// <summary>
        /// Shows the container.
        /// </summary>
        /// <param name = "startingPoint">Transform to use for position, rotation, and scale.</param>
        /// <param name = "edgeAvoidanceOverride">How far to keep the RectTransform from the edge. If null serialized avoidance will be used.</param>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public virtual void Show(Transform startingPoint, Vector2? edgeAvoidanceOverride = null)
        {
            if (startingPoint == null)
            {
                Debug.LogError($"A null Transform cannot be used as the starting point.");
                return;
            }

            Show(startingPoint.position, startingPoint.rotation, startingPoint.localScale, RectTransform.pivot);
        }

        /// <summary>
        /// Updates the rectTransform pivot.
        /// </summary>
        /// <param name = "pivot">New pivot.</param>
        /// <param name = "move">True to move the RectTransform after updating.</param>
        public virtual void UpdatePivot(Vector2 pivot, bool move = true)
        {
            RectTransform.pivot = pivot;
            if (move)
                Move();
        }

        /// <summary>
        /// Updates to a new position.
        /// </summary>
        /// <param name = "position">Next position.</param>
        /// <param name = "move">True to move towards new position.</param>
        public virtual void UpdatePosition(Vector3 position, bool move = true)
        {
            _positionGoal = position;
            if (move)
                Move();
        }

        /// <summary>
        /// Updates to a new rotation.
        /// </summary>
        /// <param name = "rotation">Next rotation.</param>
        public virtual void UpdateRotation(Quaternion rotation, bool move = true)
        {
            _rotationGoal = rotation;
            if (move)
                Move();
        }

        /// <summary>
        /// Updates to a new scale.
        /// </summary>
        /// <param name = "scale">Next scale.</param>
        /// <param name = "move">True to move the RectTransform after updating.</param>
        public virtual void UpdateScale(Vector3 scale, bool move = true)
        {
            _scaleGoal = scale;
            if (move)
                Move();
        }

        /// <summary>
        /// Updates to a new position and rotation.
        /// </summary>
        /// <param name = "position">Next position.</param>
        /// <param name = "rotation">Next rotation.</param>
        /// <param name = "move">True to move the RectTransform after updating.</param>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public virtual void UpdatePositionAndRotation(Vector3 position, Quaternion rotation, bool move = true)
        {
            UpdatePosition(position, false);
            UpdateRotation(rotation, false);
            if (move)
                Move();
        }

        /// <summary>
        /// Updates to a new position, rotation, and scale.
        /// </summary>
        /// <param name = "position">Next position.</param>
        /// <param name = "rotation">Next rotation.</param>
        /// <param name = "scale">Next scale.</param>
        /// <param name = "move">True to move the RectTransform after updating.</param>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public virtual void UpdatePositionRotationAndScale(Vector3 position, Quaternion rotation, Vector3 scale, bool move = true)
        {
            UpdatePositionAndRotation(position, rotation, false);
            UpdateScale(scale, false);
            Move();
        }

        /// <summary>
        /// Updates how much edge avoidance to use. When null serialized values are used.
        /// </summary>
        /// <param name = "edgeAvoidanceOverride">How far to keep the RectTransform from the edge. If null serialized avoidance will be used.</param>
        /// <param name = "move">True to move the RectTransform after updating.</param>
        public virtual void UpdateEdgeAvoidance(Vector2? edgeAvoidanceOverride = null, bool move = true)
        {
            _edgeAvoidance = edgeAvoidanceOverride.HasValue ? edgeAvoidanceOverride.Value : EdgeAvoidance;
            if (move)
                Move();
        }

        /// <summary>
        /// Moves to configured goals.
        /// </summary>
        protected virtual void Move()
        {
            // Update scale first so edge avoidance takes it into consideration.
            RectTransform.localScale = _scaleGoal;

            Vector2 position = _positionGoal;
            if (UseEdgeAvoidance)
            {
                Vector2 avoidance = _edgeAvoidance.HasValue ? _edgeAvoidance.Value : EdgeAvoidance;
                position = RectTransform.GetOnScreenPosition(_positionGoal, avoidance);
            }

            RectTransform.SetPositionAndRotation(position, _rotationGoal);
        }
    }
}
using FishNet.Component.Observing;
using FishNet.Managing;
using UnityEngine;

namespace FishNet.Connection
{
    /// <summary>
    /// A container for a connected client used to perform actions on and gather information for the declared client.
    /// </summary>
    public partial class NetworkConnection
    {
        #region Internal.
        /// <summary>
        /// Current GridEntry this connection is in.
        /// </summary>
        internal GridEntry HashGridEntry = HashGrid.EmptyGridEntry;
        #endregion

        #region Private.
        /// <summary>
        /// HashGrid for the NetworkManager on this connection.
        /// </summary>
        private HashGrid _hashGrid;
        /// <summary>
        /// Last unscaled time the HashGrid position was updated with this connections Objects.
        /// </summary>
        private float _nextHashGridUpdateTime;
        /// <summary>
        /// Current GridPosition this connection is in.
        /// </summary>
        private Vector2Int _hashGridPosition = HashGrid.UnsetGridPosition;
        #endregion

        /// <summary>
        /// Called when the FirstObject changes for this connection.
        /// </summary>
        private void Observers_FirstObjectChanged()
        {
            UpdateHashGridPositions(true);
        }

        /// <summary>
        /// Initializes this for use.
        /// </summary>
        private void Observers_Initialize(NetworkManager nm)
        {
            nm.TryGetInstance(out _hashGrid);
        }

        /// <summary>
        /// Updates the HashGridPosition value for FirstObject.
        /// </summary>
        internal void UpdateHashGridPositions(bool force)
        {
            if (_hashGrid == null)
                return;

            float unscaledTime = Time.unscaledTime;
            // Not enough time has passed to update.
            if (!force && unscaledTime < _nextHashGridUpdateTime)
                return;

            const float updateInterval = 1f;
            _nextHashGridUpdateTime = unscaledTime + updateInterval;

            if (FirstObject == null)
            {
                HashGridEntry = HashGrid.EmptyGridEntry;
                _hashGridPosition = HashGrid.UnsetGridPosition;
            }
            else
            {
                Vector2Int newPosition = _hashGrid.GetHashGridPosition(FirstObject);
                if (newPosition != _hashGridPosition)
                {
                    _hashGridPosition = newPosition;
                    HashGridEntry = _hashGrid.GetGridEntry(newPosition);
                }
            }
        }

        /// <summary>
        /// Resets values.
        /// </summary>
        private void Observers_Reset()
        {
            _hashGrid = null;
            _hashGridPosition = HashGrid.UnsetGridPosition;
            _nextHashGridUpdateTime = 0f;
        }
    }
}
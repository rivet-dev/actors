using System.Runtime.CompilerServices;

namespace FishNet.Managing.Timing
{
    public class EstimatedTick
    {
        #region Types.
        /// <summary>
        /// How to handle old ticks, specifically related to EstimatedTick.
        /// </summary>
        public enum OldTickOption : byte
        {
            /// <summary>
            /// Completely ignore old ticks.
            /// </summary>
            Discard = 0,
            /// <summary>
            /// Set LastRemoteTick but do not update RemoteTick.
            /// </summary>
            SetLastRemoteTick = 1,
            /// <summary>
            /// Set LastRemoteTick and RemoteTick.
            /// </summary>
            SetRemoteTick = 2
        }
        #endregion

        /// <summary>
        /// Local tick when this was last updated.
        /// </summary>
        public uint LocalTick { get; private set; } = TimeManager.UNSET_TICK;
        /// <summary>
        /// Last remote tick this was updated with that was not out of order or a duplicate.
        /// </summary>
        public uint RemoteTick { get; private set; } = TimeManager.UNSET_TICK;
        /// <summary>
        /// Last remote tick received regardless if it was out of order or a duplicate.
        /// </summary>
        public uint LastRemoteTick { get; private set; } = TimeManager.UNSET_TICK;
        /// <summary>
        /// True if LastRemoteTick is equal to RemoteTick.
        /// This would indicate that the LastRemoteTick did not arrive out of order.
        /// </summary>
        public bool IsLastRemoteTickOrdered => LastRemoteTick == RemoteTick;
        /// <summary>
        /// True if value is unset.
        /// </summary>
        // Only need to check one value for unset as they all would be if not set.
        public bool IsUnset => LocalTick == TimeManager.UNSET_TICK;
        /// <summary>
        /// Last TimeManager specified during an Update call.
        /// </summary>
        private TimeManager _updateTimeManager;
        /// <summary>
        /// LocalTick when Value was last reset.
        /// </summary>
        private uint _valueLocalTick = TimeManager.UNSET_TICK;

        /// <summary>
        /// Number of ticks LocalTick is being current LocalTick.
        /// </summary>
        public uint LocalTickDifference(TimeManager tm = null)
        {
            if (!TryAssignTimeManager(ref tm))
                return TimeManager.UNSET_TICK;

            long value = tm.LocalTick - LocalTick;
            // Shouldn't be possible to be less than 0.
            if (value < 0)
                return TimeManager.UNSET_TICK;
            else if (value > uint.MaxValue)
                value = uint.MaxValue;

            return (uint)value;
        }

        /// <summary>
        /// True if values were updated this tick.
        /// </summary>
        public bool IsCurrent(TimeManager tm = null)
        {
            if (!TryAssignTimeManager(ref tm))
                return false;

            return !IsUnset && LocalTick == tm.LocalTick;
        }

        /// <summary>
        /// Current estimated value.
        /// </summary>
        /// <param name = "nm">NetworkManager to use. When null default value will be returned.</param>
        public uint Value(TimeManager tm = null)
        {
            if (!TryAssignTimeManager(ref tm))
                return TimeManager.UNSET_TICK;

            return Value(out _, tm);
        }

        /// <summary>
        /// Current estimated value. Outputs if value is current.
        /// </summary>
        /// <param name = "nm">NetworkManager to use. When null default value will be returned.</param>
        /// <param name = "isCurrent">True if the value was updated this local tick.</param>
        public uint Value(out bool isCurrent, TimeManager tm = null)
        {
            // Default value.
            isCurrent = false;

            if (!TryAssignTimeManager(ref tm))
                return TimeManager.UNSET_TICK;
            if (IsUnset)
                return TimeManager.UNSET_TICK;

            isCurrent = IsCurrent(tm);

            uint diff = tm.LocalTick - _valueLocalTick;
            return diff + RemoteTick;
        }

        /// <summary>
        /// Initializes this EstimatedTick with values.
        /// </summary>
        public void Initialize(TimeManager tm, uint remoteTick = 0, uint lastRemoteTick = 0, uint localTick = 0)
        {
            _updateTimeManager = tm;
            RemoteTick = remoteTick;
            LastRemoteTick = lastRemoteTick;
            LocalTick = localTick;
        }

        /// <summary>
        /// Updates values.
        /// </summary>
        /// <param name = "tm">TimeManager to use.</param>
        /// <param name = "remoteTick">Remote tick being updated.</param>
        /// <param name = "oldTickOption">How to handle remoteTick if it is old.</param>
        /// ///
        /// <param name = "resetValue">True to reset Value based on this information. False will allow Value to continue to to estimate tick based on the last reset.</param>
        /// <returns>True if was able to update values.</returns>
        public bool Update(TimeManager tm, uint remoteTick, OldTickOption oldTickOption = OldTickOption.Discard, bool resetValue = true)
        {
            _updateTimeManager = tm;
            // Always set LastRemoteTick even if out of order.
            LastRemoteTick = remoteTick;
            // If cannot update with old values return.
            if (oldTickOption != OldTickOption.SetRemoteTick && remoteTick <= RemoteTick)
                return false;

            // nm is assumed set here.
            LocalTick = tm.LocalTick;
            if (resetValue)
                _valueLocalTick = LocalTick;
            RemoteTick = remoteTick;

            return true;
        }

        /// <summary>
        /// Updates values.
        /// </summary>
        /// <param name = "remoteTick">Remote tick being updated.</param>
        /// <param name = "oldTickOption">How to handle remoteTick if it is old.</param>
        /// <param name = "resetValue">True to reset Value based on this information. False will allow Value to continue to to estimate tick based on the last reset.</param>
        /// <returns>True if was able to update values.</returns>
        public bool Update(uint remoteTick, OldTickOption oldTickOption = OldTickOption.Discard, bool resetValue = true)
        {
            TimeManager tm = null;
            if (!TryAssignTimeManager(ref tm))
                return false;

            return Update(tm, remoteTick, oldTickOption);
        }

        /// <summary>
        /// Updates Value based on current ticks.
        /// This is typically used when you want to control when Value is reset through the Update methods.
        /// </summary>
        public void UpdateValue()
        {
            _valueLocalTick = LocalTick;
        }

        /// <summary>
        /// Assigns a TimeManager reference to UpdateTimeManager if was null.
        /// </summary>
        /// <returns>True if the reference has value or was assigned value. False if the reference remains null.</returns>
        private bool TryAssignTimeManager(ref TimeManager tm)
        {
            if (tm == null)
                tm = _updateTimeManager;

            return tm != null;
        }

        /// <summary>
        /// Resets values to unset and clears the NetworkManager.
        /// </summary>
        public void Reset()
        {
            ResetTicks();
            _updateTimeManager = null;
        }

        /// <summary>
        /// Resets only tick values, leaving type references.
        /// </summary>
        public void ResetTicks()
        {
            LocalTick = TimeManager.UNSET_TICK;
            RemoteTick = TimeManager.UNSET_TICK;
            LastRemoteTick = TimeManager.UNSET_TICK;
            _valueLocalTick = TimeManager.UNSET_TICK;
        }
    }
}
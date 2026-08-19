using FishNet.Managing;
using FishNet.Managing.Timing;
using System;
using UnityEngine;

namespace FishNet.Connection
{
    /// <summary>
    /// A container for a connected client used to perform actions on and gather information for the declared client.
    /// </summary>
    public partial class NetworkConnection
    {
#pragma warning disable CS0414

        #region Private.
        /// <summary>
        /// Last tick this connection sent a ping.
        /// </summary>
        private uint _lastPingTick;
        ///// <summary>
        ///// Number of times client has excessively sent a ping.
        ///// </summary>
        // private float _excessivePingCount;
        /// <summary>
        /// Ticks expected between each ping.
        /// </summary>
        private uint _requiredPingTicks;
        #endregion

        #region Const.
        /// <summary>
        /// Number of times a ping may occur excessively before server will punish connection.
        /// </summary>
        private const byte EXCESSIVE_PING_LIMIT = 10;
        #endregion

#pragma warning restore CS0414
        /// <summary>
        /// Initializes for ping.
        /// </summary>
        private void InitializePing()
        {
            // Give the client some room for error.
            float requiredInterval = NetworkManager.TimeManager.PingInterval * 0.85f;
            // Round down so required ticks is lower.
            _requiredPingTicks = NetworkManager.TimeManager.TimeToTicks(requiredInterval, TickRounding.RoundDown);
        }

        /// <summary>
        /// Resets PingPong values.
        /// </summary>
        private void ResetPingPong()
        {
            // _excessivePingCount = 0;
            _lastPingTick = 0;
        }

        /// <summary>
        /// Called when a ping is received from this connection. Returns if can respond to ping.
        /// </summary>
        /// <returns>True to respond to ping, false to kick connection.</returns>
        internal bool CanPingPong()
        {
            /* Only check ping conditions in build. Editors are prone to pausing which can
             * improperly kick clients. */
            TimeManager tm = NetworkManager == null ? InstanceFinder.TimeManager : NetworkManager.TimeManager;
            /* Server FPS is running low, timing isn't reliable enough to kick clients.
             * Respond with clients ping and remove infractions just in case the
             * client received some from other server instabilities. */
            if (tm.HasMultipleTicksOccurred(timeSinceMultipleTicks: 1f))
            {
                // _excessivePingCount = 0f;
                return false;
            }

            uint currentTick = tm.Tick;
            uint difference = currentTick - _lastPingTick;
            _lastPingTick = currentTick;

            // Ping sent too quickly.
            if (difference < _requiredPingTicks)
            {
                // _excessivePingCount += 1f;
                ////Ping limit hit.
                // if (_excessivePingCount >= EXCESSIVE_PING_LIMIT)
                // {
                //    NetworkManager.LogWarning($"Kicked connectionId {ClientId} for excessive pings.");
                //    Disconnect(true);
                // }

                // Return to not send pong back.
                return false;
            }
            // Ping isnt too fast.
            else
            {
                // _excessivePingCount = UnityEngine.Mathf.Max(0f, _excessivePingCount - 0.5f);
                return true;
            }
        }
    }
}
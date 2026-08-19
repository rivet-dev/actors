using System;
using FishNet.Serializing;
using GameKit.Dependencies.Utilities;

namespace FishNet.Managing.Timing
{
    public readonly struct PreciseTick : IEquatable<PreciseTick>
    {
        /// <summary>
        /// The current tick.
        /// </summary>
        public readonly uint Tick;
        /// <summary>
        /// Percentage of the tick returned between 0d and 1d.
        /// </summary>
        public readonly double PercentAsDouble;
        /// <summary>
        /// Percentage of the tick returned between 0 and 100.
        /// </summary>
        public readonly byte PercentAsByte;
        /// <summary>
        /// Maximum value a percent can be as a double.
        /// </summary>
        public const double MAXIMUM_DOUBLE_PERCENT = 1d;
        /// <summary>
        /// Maximum value a percent can be as a byte.
        /// </summary>
        public const byte MAXIMUM_BYTE_PERCENT = 100;

        /// <summary>
        /// Value to use when a precise tick is unset.
        /// </summary>
        public static PreciseTick GetUnsetValue() => new(TimeManager.UNSET_TICK, (byte)0);

        /// <summary>
        /// Creates a precise tick where the percentage is 0.
        /// </summary>
        public PreciseTick(uint tick)
        {
            Tick = tick;
            PercentAsByte = 0;
            PercentAsDouble = 0d;
        }

        /// <summary>
        /// Creates a precise tick where the percentage is a byte between 0 and 100.
        /// </summary>
        public PreciseTick(uint tick, byte percentAsByte)
        {
            Tick = tick;

            percentAsByte = Maths.ClampByte(percentAsByte, 0, MAXIMUM_BYTE_PERCENT);
            PercentAsByte = percentAsByte;
            PercentAsDouble = percentAsByte / 100d;
        }

        /// <summary>
        /// Creates a precise tick where the percentage is a double between 0d and 1d.
        /// </summary>
        public PreciseTick(uint tick, double percent)
        {
            Tick = tick;
            percent = Maths.ClampDouble(percent, 0d, MAXIMUM_DOUBLE_PERCENT);
            PercentAsByte = (byte)(percent * 100d);
            PercentAsDouble = percent;
        }

        public bool IsValid() => Tick != TimeManager.UNSET_TICK;

        /// <summary>
        /// Prints PreciseTick information as a string.
        /// </summary>
        /// <returns></returns>
        public override string ToString() => $"Tick {Tick}, Percent {PercentAsByte.ToString("000")}";

        public static bool operator ==(PreciseTick a, PreciseTick b)
        {
            return a.Tick == b.Tick && a.PercentAsByte == b.PercentAsByte;
        }

        public static bool operator !=(PreciseTick a, PreciseTick b)
        {
            return !(a == b);
        }

        public static bool operator >=(PreciseTick a, PreciseTick b)
        {
            if (b.Tick > a.Tick)
                return false;
            if (a.Tick > b.Tick)
                return true;
            // If here ticks are the same.
            return a.PercentAsByte >= b.PercentAsByte;
        }

        public static bool operator <=(PreciseTick a, PreciseTick b) => b >= a;

        public static bool operator >(PreciseTick a, PreciseTick b)
        {
            if (b.Tick > a.Tick)
                return false;
            if (a.Tick > b.Tick)
                return true;
            // if here ticks are the same.
            return a.PercentAsByte > b.PercentAsByte;
        }

        public static bool operator <(PreciseTick a, PreciseTick b) => b > a;
        public bool Equals(PreciseTick other) => Tick == other.Tick && PercentAsByte == other.PercentAsByte;
        public override bool Equals(object obj) => obj is PreciseTick other && Equals(other);
        public override int GetHashCode() => HashCode.Combine(Tick, PercentAsDouble, PercentAsByte);
    }

    public static class PreciseTickExtensions
    {
        /// <summary>
        /// Adds value onto a PreciseTick.
        /// </summary>
        /// <param name = "value">Value to add.</param>
        /// <param name = "delta">Tick delta.</param>
        /// <returns></returns>
        public static PreciseTick Add(this PreciseTick pt, PreciseTick value, double delta)
        {
            double ptDouble = pt.AsDouble(delta);
            double valueDouble = value.AsDouble(delta);

            double next = ptDouble + valueDouble;

            return next.AsPreciseTick(delta);
        }

        /// <summary>
        /// Subtracts value from a PreciseTick.
        /// </summary>
        /// <param name = "value">Value to subtract.</param>
        /// <param name = "delta">Tick delta.</param>
        /// <returns></returns>
        public static PreciseTick Subtract(this PreciseTick pt, PreciseTick value, double delta)
        {
            double ptDouble = pt.AsDouble(delta);
            double valueDouble = value.AsDouble(delta);

            double remainder = ptDouble - valueDouble;

            return remainder.AsPreciseTick(delta);
        }

        /// <summary>
        /// Converts a PreciceTick to a double.
        /// </summary>
        /// <param name = "delta">Tick delta.</param>
        /// <returns></returns>
        public static double AsDouble(this PreciseTick pt, double delta)
        {
            return (double)pt.Tick * delta + pt.PercentAsDouble * delta;
        }

        /// <summary>
        /// Converts a double to a PreciseTick.
        /// </summary>
        /// <param name = "delta">Tick delta.</param>
        /// <returns></returns>
        public static PreciseTick AsPreciseTick(this double ptDouble, double delta)
        {
            if (ptDouble <= 0)
                return new(0, 0);

            ulong whole = (ulong)Math.Floor(ptDouble / delta);
            // Overflow.
            if (whole >= uint.MaxValue)
                return PreciseTick.GetUnsetValue();

            double remainder = ptDouble % delta;

            double percent = remainder / delta;
            return new((uint)whole, percent);
        }
    }

    public static class PreciseTickSerializer
    {
        public static void WritePreciseTick(this Writer writer, PreciseTick value)
        {
            writer.WriteTickUnpacked(value.Tick);
            writer.WriteUInt8Unpacked(value.PercentAsByte);
        }

        public static PreciseTick ReadPreciseTick(this Reader reader)
        {
            uint tick = reader.ReadTickUnpacked();
            byte percentByte = reader.ReadUInt8Unpacked();
            return new(tick, percentByte);
        }
    }
}
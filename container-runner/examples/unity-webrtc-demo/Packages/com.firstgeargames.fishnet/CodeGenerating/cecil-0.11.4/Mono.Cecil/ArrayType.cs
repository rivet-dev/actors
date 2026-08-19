//
// Author:
//   Jb Evain (jbevain@gmail.com)
//
// Copyright (c) 2008 - 2015 Jb Evain
// Copyright (c) 2008 - 2011 Novell, Inc.
//
// Licensed under the MIT/X11 license.
//

using MonoFN.Collections.Generic;
using System;
using System.Text;
using System.Threading;
using MD = MonoFN.Cecil.Metadata;

namespace MonoFN.Cecil
{
    public struct ArrayDimension
    {
        private int? lower_bound;
        private int? upper_bound;
        public int? LowerBound
        {
            get { return lower_bound; }
            set { lower_bound = value; }
        }
        public int? UpperBound
        {
            get { return upper_bound; }
            set { upper_bound = value; }
        }
        public bool IsSized
        {
            get { return lower_bound.HasValue || upper_bound.HasValue; }
        }

        public ArrayDimension(int? lowerBound, int? upperBound)
        {
            lower_bound = lowerBound;
            upper_bound = upperBound;
        }

        public override string ToString()
        {
            return !IsSized ? string.Empty : lower_bound + "..." + upper_bound;
        }
    }

    public sealed class ArrayType : TypeSpecification
    {
        private Collection<ArrayDimension> dimensions;
        public Collection<ArrayDimension> Dimensions
        {
            get
            {
                if (dimensions != null)
                    return dimensions;

                Collection<ArrayDimension> empty_dimensions = new();
                empty_dimensions.Add(new());

                Interlocked.CompareExchange(ref dimensions, empty_dimensions, null);

                return dimensions;
            }
        }
        public int Rank
        {
            get { return dimensions == null ? 1 : dimensions.Count; }
        }
        public bool IsVector
        {
            get
            {
                if (dimensions == null)
                    return true;

                if (dimensions.Count > 1)
                    return false;

                ArrayDimension dimension = dimensions[0];

                return !dimension.IsSized;
            }
        }
        public override bool IsValueType
        {
            get { return false; }
            set { throw new InvalidOperationException(); }
        }
        public override string Name
        {
            get { return base.Name + Suffix; }
        }
        public override string FullName
        {
            get { return base.FullName + Suffix; }
        }
        private string Suffix
        {
            get
            {
                if (IsVector)
                    return "[]";

                StringBuilder suffix = new();
                suffix.Append("[");
                for (int i = 0; i < dimensions.Count; i++)
                {
                    if (i > 0)
                        suffix.Append(",");

                    suffix.Append(dimensions[i].ToString());
                }
                suffix.Append("]");

                return suffix.ToString();
            }
        }
        public override bool IsArray
        {
            get { return true; }
        }

        public ArrayType(TypeReference type) : base(type)
        {
            Mixin.CheckType(type);
            etype = MD.ElementType.Array;
        }

        public ArrayType(TypeReference type, int rank) : this(type)
        {
            Mixin.CheckType(type);

            if (rank == 1)
                return;

            dimensions = new(rank);
            for (int i = 0; i < rank; i++)
                dimensions.Add(new());
            etype = MD.ElementType.Array;
        }
    }
}
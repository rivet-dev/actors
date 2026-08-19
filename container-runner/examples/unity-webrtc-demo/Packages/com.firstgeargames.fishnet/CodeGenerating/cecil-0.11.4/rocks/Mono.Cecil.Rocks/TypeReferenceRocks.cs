//
// Author:
//   Jb Evain (jbevain@gmail.com)
//
// Copyright (c) 2008 - 2015 Jb Evain
// Copyright (c) 2008 - 2011 Novell, Inc.
//
// Licensed under the MIT/X11 license.
//

using System;

namespace MonoFN.Cecil.Rocks
{
#if UNITY_EDITOR
    public
#endif
        static class TypeReferenceRocks
    {
        public static ArrayType MakeArrayType(this TypeReference self)
        {
            return new(self);
        }

        public static ArrayType MakeArrayType(this TypeReference self, int rank)
        {
            if (rank == 0)
                throw new ArgumentOutOfRangeException("rank");

            ArrayType array = new(self);

            for (int i = 1; i < rank; i++)
                array.Dimensions.Add(new());

            return array;
        }

        public static PointerType MakePointerType(this TypeReference self)
        {
            return new(self);
        }

        public static ByReferenceType MakeByReferenceType(this TypeReference self)
        {
            return new(self);
        }

        public static OptionalModifierType MakeOptionalModifierType(this TypeReference self, TypeReference modifierType)
        {
            return new(modifierType, self);
        }

        public static RequiredModifierType MakeRequiredModifierType(this TypeReference self, TypeReference modifierType)
        {
            return new(modifierType, self);
        }

        public static GenericInstanceType MakeGenericInstanceType(this TypeReference self, params TypeReference[] arguments)
        {
            if (self == null)
                throw new ArgumentNullException("self");
            if (arguments == null)
                throw new ArgumentNullException("arguments");
            if (arguments.Length == 0)
                throw new ArgumentException();
            if (self.GenericParameters.Count != arguments.Length)
                throw new ArgumentException();

            GenericInstanceType instance = new(self, arguments.Length);

            foreach (TypeReference argument in arguments)
                instance.GenericArguments.Add(argument);

            return instance;
        }

        public static PinnedType MakePinnedType(this TypeReference self)
        {
            return new(self);
        }

        public static SentinelType MakeSentinelType(this TypeReference self)
        {
            return new(self);
        }
    }
}
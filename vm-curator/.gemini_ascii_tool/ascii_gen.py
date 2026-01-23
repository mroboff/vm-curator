import requests
import sys
import argparse
from PIL import Image, ImageOps
from io import BytesIO

# Simplified chars to avoid syntax errors
ASCII_CHARS = r"@%#*+=-:. "

def resize_image(image, new_width=100, ratio_val=2.0):
    width, height = image.size
    ratio = height / width / ratio_val
    new_height = int(new_width * ratio)
    if new_height == 0: new_height = 1
    resized_image = image.resize((new_width, new_height), resample=Image.LANCZOS)
    return resized_image

def grayify(image):
    return image.convert("L")

def pixels_to_ascii(image):
    pixels = image.getdata()
    ascii_str_len = len(ASCII_CHARS)
    characters = "".join([ASCII_CHARS[int(pixel * (ascii_str_len - 1) / 255)] for pixel in pixels])
    return characters

def generate_ascii(url, width, ratio, threshold=None):
    try:
        headers = {'User-Agent': 'Mozilla/5.0'}
        response = requests.get(url, headers=headers)
        response.raise_for_status()
        img = Image.open(BytesIO(response.content))
    except Exception as e:
        print(f"Error loading image: {e}")
        return

    if img.mode == 'P':
        img = img.convert('RGBA')

    if img.mode in ('RGBA', 'LA'):
        # Force to RGB with white background
        background = Image.new('RGB', img.size, (255, 255, 255))
        if img.mode == 'RGBA':
            background.paste(img, mask=img.split()[3])
        else:
            background.paste(img, mask=img.split()[1])
        img = background
    
    img = grayify(resize_image(img, width, ratio))
    
    if threshold is not None:
        img = img.point(lambda p: 255 if p > threshold else 0)
    
    new_image_data = pixels_to_ascii(img)
    
    pixel_count = len(new_image_data)
    ascii_image = "\n".join([new_image_data[index:(index+width)] for index in range(0, pixel_count, width)])
    return ascii_image

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description='Generate ASCII art from an image URL.')
    parser.add_argument('url', type=str, help='URL of the image')
    parser.add_argument('--width', type=int, default=80, help='Width of the ASCII art')
    parser.add_argument('--ratio', type=float, default=2.0, help='Aspect ratio correction')
    parser.add_argument('--threshold', type=int, default=None, help='Threshold for binarization (0-255)')
    
    args = parser.parse_args()
    
    result = generate_ascii(args.url, args.width, args.ratio, args.threshold)
    if result:
        print(result)

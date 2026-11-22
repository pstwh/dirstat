pub type Rect = [f64; 4];

fn worst(sum: f64, largest: f64, smallest: f64, side: f64, scale: f64) -> f64 {
    let thickness = sum * scale / side;
    if thickness <= 0.0 {
        return f64::INFINITY;
    }
    let ratio = |weight: f64| {
        let length = weight * scale / thickness;
        if length <= 0.0 {
            f64::INFINITY
        } else {
            (thickness / length).max(length / thickness)
        }
    };
    ratio(largest).max(ratio(smallest))
}

pub fn squarify(weights: &[f64], rect: Rect, min_area: f64, out: &mut Vec<Rect>) -> Option<Rect> {
    out.clear();
    let mut remaining: f64 = weights.iter().sum();
    let [mut x0, mut y0, x1, y1] = rect;
    let mut index = 0;
    while index < weights.len() {
        let width = x1 - x0;
        let height = y1 - y0;
        if width <= 0.0 || height <= 0.0 || remaining <= 0.0 {
            return None;
        }
        let scale = width * height / remaining;
        if weights[index] * scale < min_area {
            return Some([x0, y0, x1, y1]);
        }
        let vertical = width >= height;
        let side = if vertical { height } else { width };
        let mut end = index + 1;
        let mut sum = weights[index];
        let mut current = worst(sum, weights[index], weights[index], side, scale);
        while end < weights.len() {
            let next_sum = sum + weights[end];
            let next = worst(next_sum, weights[index], weights[end], side, scale);
            if next > current {
                break;
            }
            sum = next_sum;
            current = next;
            end += 1;
        }
        let last_row = end == weights.len();
        let thickness = if last_row {
            if vertical { width } else { height }
        } else {
            sum * scale / side
        };
        let mut position = if vertical { y0 } else { x0 };
        let limit = if vertical { y1 } else { x1 };
        for (offset, weight) in weights[index..end].iter().enumerate() {
            let length = weight / sum * side;
            let next = if index + offset + 1 == end {
                limit
            } else {
                position + length
            };
            if vertical {
                out.push([x0, position, x0 + thickness, next]);
            } else {
                out.push([position, y0, next, y0 + thickness]);
            }
            position = next;
        }
        if vertical {
            x0 += thickness;
        } else {
            y0 += thickness;
        }
        remaining -= sum;
        index = end;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(r: &Rect) -> f64 {
        (r[2] - r[0]) * (r[3] - r[1])
    }

    #[test]
    fn paper_example_fills_the_rect_proportionally() {
        let weights = [6.0, 6.0, 4.0, 3.0, 2.0, 2.0, 1.0];
        let mut out = Vec::new();
        let rest = squarify(&weights, [0.0, 0.0, 6.0, 4.0], 0.0, &mut out);
        assert!(rest.is_none());
        assert_eq!(out.len(), 7);
        for (rect, weight) in out.iter().zip(weights) {
            assert!((area(rect) - weight).abs() < 1e-9, "{rect:?} {weight}");
        }
        let total: f64 = out.iter().map(area).sum();
        assert!((total - 24.0).abs() < 1e-9);
        assert_eq!(out[0], [0.0, 0.0, 3.0, 2.0]);
        assert_eq!(out[1], [0.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn rects_do_not_overlap() {
        let weights: Vec<f64> = (1..200).rev().map(|v| v as f64).collect();
        let mut out = Vec::new();
        squarify(&weights, [0.0, 0.0, 800.0, 600.0], 0.0, &mut out);
        for (i, a) in out.iter().enumerate() {
            assert!(a[0] >= 0.0 && a[1] >= 0.0 && a[2] <= 800.0 + 1e-6 && a[3] <= 600.0 + 1e-6);
            for b in &out[i + 1..] {
                let overlap_x = a[2].min(b[2]) - a[0].max(b[0]);
                let overlap_y = a[3].min(b[3]) - a[1].max(b[1]);
                assert!(overlap_x <= 1e-6 || overlap_y <= 1e-6);
            }
        }
    }

    #[test]
    fn tiny_items_are_lumped() {
        let mut weights = vec![1000.0];
        weights.extend(std::iter::repeat_n(0.0001, 1000));
        let mut out = Vec::new();
        let rest = squarify(&weights, [0.0, 0.0, 100.0, 100.0], 0.5, &mut out);
        assert_eq!(out.len(), 1);
        assert!(rest.is_some());
    }
}
